// The player's actions (the Java `PlayerService`): one player for the whole
// app, so it keeps sounding while moving between screens. What to play next
// is the `PlayerQueue`'s; what actually sounds is the PlayerBackend behind
// `api` (the webview's media element in the app, a clock in the mock).
//
// The last file and its second are kept between runs (`PlayerMemory`): at
// startup it comes back to the bar PAUSED where it was left.

import { api, playerBackend as engine, type ConnectQueueItem, type ConnectCast, type ConnectCastItem, type ConnectCommand, type PlayerMemory, type Track } from './api';
import { findBlock, onDisk } from './library';
import { PlayerQueue, nextRepeat } from './playerQueue';
import { emit, ui } from './state';
import { orderOf, sortTracks } from './trackOrder';

/**
 * What the built-in player can open, by format NAME (ALAC is .m4a too and
 * does not play). Tried one by one on WebView2: AVI and FLV do not open in
 * any codec yt-dlp leaves them in, and they are not converted (decided: the
 * rest of the list is listed but dimmed, with the reason).
 */
export const PLAYABLE: ReadonlySet<string> = new Set([
  'MP3', 'OPUS', 'M4A', 'FLAC', 'AAC', 'VORBIS', 'WAV', 'MP4', 'MKV', 'MOV', 'WEBM',
]);

/** Files in a row that may fail before it stops: with a list of hundreds, going through them all skipped dozens of songs in seconds. */
export const MAX_SKIPS = 3;
const RETRY_DELAY_MS = 600;

/**
 * What to do when the file that sounds fails: once more the same one (a
 * transient error), then the next, and stop when the whole list has failed or
 * `MAX_SKIPS` have failed one after another.
 */
export function afterFailure(state: { failures: number; queueSize: number; alreadyRetried: boolean }): 'retry' | 'skip' | 'stop' {
  if (!state.alreadyRetried) return 'retry';
  if (state.failures + 1 >= Math.min(state.queueSize, MAX_SKIPS)) return 'stop';
  return 'skip';
}

let queue: PlayerQueue<Track> | null = null;
let currentBlock = '';
// Files that failed one after another: when all of them have, it stops
// instead of going round the list for ever.
let failures = 0;
// The file that was already tried again after failing, so it is tried once only.
let retriedId = '';
// The second to jump to once the file is open; before that a seek is ignored.
let pendingSeek = 0;
let memory: PlayerMemory = { volume: ui.volume, last: null };

/**
 * «Escuchar en»: another device passed what it plays and it sounds here, read from it (the back's relay). Its queue is
 * `CAST_KEY`: nothing of the library, nothing kept in `PlayerMemory` (the last file of THIS device stays the one it was).
 */
export const CAST_KEY = 'cast:';
let cast: { from: string; playlist: string; tracks: Track[]; items: Map<string, ConnectCastItem> } | null = null;
let castLost: (from: string) => void = () => {};

engine.onTime(seconds => {
  ui.position = seconds;
  emit('player');
});
engine.onDuration(seconds => {
  failures = 0;
  retriedId = '';
  ui.duration = seconds;
  if (pendingSeek > 0 && pendingSeek < seconds - 1) seek(pendingSeek);
  pendingSeek = 0;
  emit('player');
});
engine.onEnded(() => {
  if (!queue) return;
  if (queue.repeat === 'one') {
    seek(0);
    setPlaying(true);
    return;
  }
  const after = queue.afterEnd();
  if (after) {
    load(after, true);
  } else {
    // End of the list: it stays loaded at the start, like every player.
    setPlaying(false);
    seek(0);
  }
});
engine.onError(() => {
  if (!queue || !ui.nowPlaying) return;
  const action = afterFailure({ failures, queueSize: queue.size, alreadyRetried: retriedId === ui.nowPlaying.track.id });
  // Another device's file that fails twice: that device is gone (or asleep), and the next files would fail the same.
  if (cast && action !== 'retry') {
    setPlaying(false);
    castLost(cast.from);
    return;
  }
  if (action === 'retry') {
    // Often a hiccup (the disk busy, the asset protocol late): the same file once more, at the same second.
    const track = ui.nowPlaying.track;
    retriedId = track.id;
    const at = ui.position;
    window.setTimeout(() => {
      if (ui.nowPlaying?.track.id === track.id) load(track, ui.playing, at);
    }, RETRY_DELAY_MS);
    return;
  }
  failures++;
  if (action === 'stop') stop();
  else load(queue.next(), true);
});

// Pause, play, next and previous from the phone's notification, lock screen or headphones.
engine.onPlayState?.(playing => {
  if (ui.remote || !ui.nowPlaying || ui.playing === playing) return;
  ui.playing = playing;
  emit('player');
  if (!playing) void savePosition();
});
// The device's volume moved (its buttons): the slider follows at once.
engine.onVolume?.(volume => {
  if (ui.remote || ui.volume === volume) return;
  ui.volume = volume;
  memory.volume = volume;
  emit('player');
});
engine.onRemoteSkip?.(direction => {
  if (direction === 'next') next();
  else previous();
});
// Shuffle and repeat pressed in the notification.
engine.onRemoteMode?.(mode => {
  if (mode === 'shuffle') toggleShuffle();
  else cycleRepeat();
});

/** The app was opened from the phone's music notification: the screen should show what is playing. */
export function onOpenRequest(listener: () => void): void {
  engine.onRemoteOpen?.(listener);
}

// === PUBLIC API ===

/**
 * Soundify Connect: while another device is being controlled, the player's actions go to it
 * instead of the local engine (`connect.ts` sets this and puts that device's state in `ui`).
 */
let remoteRoute: ((command: ConnectCommand) => void) | null = null;
/**
 * Playing something of THIS device while another is controlled: control of the other is let go of (the connection to it
 * stays, to take it again whenever) and this device plays. `connect.ts` gives it with `enterRemote`.
 */
let releaseRemote: (() => void) | null = null;

/** True if nothing remote is in the way any more (it was let go of, or there was nothing). */
function takeBackLocal(): boolean {
  if (!remoteRoute) return true;
  const release = releaseRemote;
  release?.();
  return !remoteRoute;
}

/** What the local player was doing when another device took over the screen. */
export interface LocalSnapshot {
  nowPlaying: typeof ui.nowPlaying;
  position: number;
  duration: number;
  volume: number;
  shuffle: boolean;
  repeat: typeof ui.repeat;
}

/** Starts controlling another device: the local one is paused and kept as it is, to come back to it. */
export function enterRemote(route: (command: ConnectCommand) => void, release: () => void): LocalSnapshot {
  if (ui.playing) setPlaying(false);
  void savePosition();
  const snapshot: LocalSnapshot = { nowPlaying: ui.nowPlaying, position: ui.position, duration: ui.duration, volume: ui.volume, shuffle: ui.shuffle, repeat: ui.repeat };
  remoteRoute = route;
  releaseRemote = release;
  return snapshot;
}

/** Back to the local player, paused where it was. */
export function leaveRemote(snapshot: LocalSnapshot): void {
  remoteRoute = null;
  releaseRemote = null;
  ui.nowPlaying = snapshot.nowPlaying;
  ui.playing = false;
  ui.position = snapshot.position;
  ui.duration = snapshot.duration;
  ui.volume = snapshot.volume;
  ui.shuffle = snapshot.shuffle;
  ui.repeat = snapshot.repeat;
  emit('player');
}

/** The files of a playlist of this device's library that can be played, in the order the Reproductor shows them (a controller browsing it). */
export function browseQueue(blockKey: string): { playlist: string; items: ConnectQueueItem[] } | null {
  const found = findBlock(blockKey);
  if (!found) return null;
  // ALL its files, in the order of the Reproductor: with when each arrived and, if it is not on disk, why it cannot be played.
  const items = sortTracks(found.block.tracks, orderOf(blockKey)).map(tr => ({
    videoId: tr.id, title: tr.title, addedDaysAgo: tr.addedDaysAgo, status: tr.status === 'ok' ? null : tr.status,
  }));
  return { playlist: found.playlist.title, items };
}

/** A controller picked a file of a playlist of this device's library: it plays from there. False if the file is not there (any more). */
export function playFromBlock(blockKey: string, videoId: string): boolean {
  const index = queueOf(blockKey).findIndex(tr => tr.id === videoId);
  return index >= 0 && start(blockKey, index, true);
}

/** The files of the block that sounds, in the order it plays them, for handing the playback to another device. */
export function currentQueue(): { playlist: string; items: ConnectQueueItem[]; index: number } | null {
  if (!ui.nowPlaying || !currentBlock) return null;
  const tracks = queueOf(currentBlock);
  const index = tracks.findIndex(tr => tr.id === ui.nowPlaying?.track.id);
  if (index < 0) return null;
  return { playlist: playlistTitle(), items: tracks.map(tr => ({ videoId: tr.id, title: tr.title, addedDaysAgo: tr.addedDaysAgo })), index };
}

/**
 * «Escuchar en»: `from` passed what it plays. It sounds here from the same file and second, in the order it had (already
 * shuffled there, so «next» is the same file on both sides). What this device played is left where it was.
 */
export function playCast(from: string, pass: ConnectCast, nativeBase?: string): boolean {
  const items = pass.items.filter(item => item.videoId);
  if (items.length === 0 || pass.index < 0 || pass.index >= items.length) return false;
  void savePosition();
  const tracks = castTracks(items, nativeBase);
  cast = { from, playlist: pass.playlist, tracks, items: new Map(items.map(item => [item.videoId, item])) };
  ui.castFrom = from;
  ui.shuffle = pass.shuffle;
  ui.repeat = pass.repeat;
  queue = new PlayerQueue(tracks, pass.index);
  queue.repeat = pass.repeat;
  currentBlock = CAST_KEY;
  failures = 0;
  retriedId = '';
  load(queue.current(), pass.playing, pass.position);
  return true;
}

/**
 * The page started while a pass sounded here (it only happens on Android, where the audio is the native player's and goes
 * on without the page): the queue, the file and the second come back from what the back remembers and this device last
 * published, and nothing is loaded: it is already playing.
 */
async function resumeCast(): Promise<boolean> {
  const current = await api.connectCastCurrent().catch(() => null);
  if (!current?.castBase || current.pass.items.length === 0) return false;
  const { pass, state } = current;
  const tracks = castTracks(pass.items, current.castBase);
  const found = tracks.findIndex(track => track.id === state.nowPlaying?.videoId);
  const index = found >= 0 ? found : Math.min(pass.index, tracks.length - 1);
  cast = { from: current.from, playlist: pass.playlist, tracks, items: new Map(pass.items.map(item => [item.videoId, item])) };
  ui.castFrom = current.from;
  ui.shuffle = state.shuffle;
  ui.repeat = state.repeat;
  queue = new PlayerQueue(tracks, index);
  queue.repeat = state.repeat;
  currentBlock = CAST_KEY;
  const track = tracks[index] as Track;
  ui.nowPlaying = { blockKey: CAST_KEY, track };
  ui.position = state.position;
  ui.duration = state.duration || track.seconds;
  ui.playing = state.playing;
  engine.setModes?.(ui.shuffle, ui.repeat);
  emit('player');
  return true;
}

function castTracks(items: ConnectCastItem[], nativeBase?: string): Track[] {
  return items.map((item, n) => ({
    id: item.videoId, number: n + 1, title: item.title, addedDaysAgo: 0, seconds: item.duration, status: 'ok', file: null,
    // Android: audio through the native player (it goes on with the screen off) by the local relay; video through the webview.
    url: nativeBase && !item.video ? `${nativeBase}media/${encodeURIComponent(item.videoId)}` : api.castUrl(item.videoId, 'media'),
  }));
}

/** The device that passed it took it back (or stopped it): nothing of it sounds here any more. */
export function endCast(): void {
  if (!cast) return;
  stop();
}

/**
 * «Escuchar en», the device that passes: what plays here, in the order it plays (shuffled as it is), with where each
 * file is on THIS device (the back decides what of it may be read). Null when there is nothing of this device to pass.
 */
export function passableQueue(): {
  playlist: string; index: number;
  items: Array<{ videoId: string; title: string; duration: number; format: string; video: boolean; file: string }>;
} | null {
  const now = ui.nowPlaying;
  if (cast || !queue || !now || !currentBlock) return null;
  const found = findBlock(currentBlock);
  if (!found) return null;
  const items = queue.inOrder().items.filter(track => track.file).map(track => ({
    videoId: track.id,
    title: track.title,
    duration: track.id === now.track.id ? ui.duration || track.seconds : track.seconds,
    format: extensionOf(track.file ?? '') || found.block.format.toLowerCase(),
    video: found.block.video,
    file: track.file ?? '',
  }));
  const index = items.findIndex(item => item.videoId === now.track.id);
  return index < 0 ? null : { playlist: found.playlist.title, index, items };
}

function extensionOf(path: string): string {
  const dot = path.lastIndexOf('.');
  return dot > path.lastIndexOf('/') && dot > path.lastIndexOf('\\') ? path.slice(dot + 1).toLowerCase() : '';
}

/** Is what sounds another device's, and whose (its name)? */
export function castHere(): { from: string; playlist: string } | null {
  return cast ? { from: cast.from, playlist: cast.playlist } : null;
}

/** What Connect says of the file that sounds when it is another device's (its playlist, format and kind). */
export function castBlockInfo(): { playlist: string; format: string; video: boolean } | null {
  const item = cast && ui.nowPlaying ? cast.items.get(ui.nowPlaying.track.id) : undefined;
  return cast && item ? { playlist: cast.playlist, format: item.format.toUpperCase(), video: item.video } : null;
}

/** The picture of a file passed here, if the other device has one. */
export function castCoverUrl(videoId: string): string | null {
  return cast?.items.get(videoId)?.cover ? api.castUrl(videoId, 'cover') : null;
}

/** Another device's files stopped answering (`connect.ts` says so). */
export function onCastLost(listener: (from: string) => void): void {
  castLost = listener;
}

/**
 * Soundify Connect: a controller picked a file of the playlist that sounds here (it only knows the video id, which means
 * the same on every device). It plays from the start. False if the file is not in this playlist.
 */
export function playTrackById(videoId: string, at = 0): boolean {
  if (!currentBlock) return false;
  const index = queueOf(currentBlock).findIndex(tr => tr.id === videoId);
  if (index < 0) return false;
  // What another device passed is no block of the library: its queue is set again at that file.
  if (cast && currentBlock === CAST_KEY) {
    queue = new PlayerQueue(cast.tracks, index);
    queue.repeat = ui.repeat;
    failures = 0;
    load(queue.current(), true, at);
    return true;
  }
  return start(currentBlock, index, true, at);
}

export function isPlayable(format: string | null | undefined): boolean {
  return !!format && PLAYABLE.has(format.toUpperCase());
}

/** Plays `track` of a block, with the block's files on disk as the queue. */
export function play(blockKey: string, track: Track): void {
  if (!takeBackLocal()) return;
  const index = queueOf(blockKey).findIndex(tr => tr.id === track.id);
  if (index >= 0) start(blockKey, index, true);
}

/**
 * The list of the playing block was sorted again: the queue follows the new
 * order from the file that is sounding, which keeps playing untouched.
 */
export function reorderQueue(blockKey: string): void {
  if (!queue || currentBlock !== blockKey || !ui.nowPlaying) return;
  const tracks = queueOf(blockKey);
  const playing = ui.nowPlaying.track.id;
  const index = tracks.findIndex(tr => tr.id === playing);
  if (index < 0) return;
  queue = new PlayerQueue(tracks, index);
  queue.repeat = ui.repeat;
  if (ui.shuffle) queue.setShuffled(true);
}

/** From the first file (shuffled, the rest come in a fixed random order). */
export function playAll(blockKey: string): void {
  if (!takeBackLocal()) return;
  start(blockKey, 0, true);
}

export function togglePlay(): void {
  if (remoteRoute) return remoteRoute({ cmd: 'toggle' });
  if (!ui.nowPlaying) return;
  setPlaying(!ui.playing);
  if (!ui.playing) void savePosition();
}

export function next(): void {
  if (remoteRoute) return remoteRoute({ cmd: 'next' });
  if (!queue) return;
  failures = 0;
  load(queue.next(), true);
}

export function previous(): void {
  if (remoteRoute) return remoteRoute({ cmd: 'previous' });
  if (!queue) return;
  // Like every player: past the first seconds, "previous" restarts the file.
  if (ui.position > 3) {
    seek(0);
    return;
  }
  failures = 0;
  load(queue.previous(), true);
}

export function seek(seconds: number): void {
  if (remoteRoute) return remoteRoute({ cmd: 'seek', seconds });
  ui.position = seconds;
  engine.seek(seconds);
  emit('player');
}

export function toggleShuffle(): void {
  if (remoteRoute) return remoteRoute({ cmd: 'shuffle' });
  ui.shuffle = !ui.shuffle;
  queue?.setShuffled(ui.shuffle);
  engine.setModes?.(ui.shuffle, ui.repeat);
  emit('player');
}

export function cycleRepeat(): void {
  if (remoteRoute) return remoteRoute({ cmd: 'repeat' });
  ui.repeat = nextRepeat(ui.repeat);
  if (queue) queue.repeat = ui.repeat;
  engine.setModes?.(ui.shuffle, ui.repeat);
  emit('player');
}

/** While the slider moves; `saveVolume` once it is let go. */
export function setVolume(volume: number): void {
  if (remoteRoute) {
    ui.volume = volume;
    emit('player');
    return remoteRoute({ cmd: 'volume', volume });
  }
  ui.volume = volume;
  engine.setVolume(volume);
  emit('player');
}

export function saveVolume(): void {
  if (remoteRoute) return;
  memory.volume = ui.volume;
  void persist();
}

/** The bar's ✕: stops and forgets the last file, so it does not come back at startup. */
export function close(): void {
  stop();
  memory.last = null;
  void persist();
}

/** Saves the second it is at: on every pause and when the app closes. */
export function savePosition(): Promise<void> {
  // What shows while another device is controlled is not this player's, and neither is what another device passed here.
  if (ui.remote || cast) return Promise.resolve();
  if (memory.last && ui.nowPlaying) memory.last.position = ui.position;
  return persist();
}

/**
 * At startup: the saved volume, and the last file back in the bar, paused at
 * its second. If it is not on disk any more there is simply no bar.
 */
export async function restoreLast(): Promise<void> {
  memory = await api.playerMemory().catch(() => memory);
  // A pass from another device was sounding here when the page started (Android closed the window and kept the player's
  // service): the screen takes it back up instead of showing this device's last file, which would also load over it.
  if (await resumeCast()) return;
  // When the volume is the device's it is not ours to set at startup: the slider shows where it is.
  if (!engine.externalVolume) {
    ui.volume = memory.volume;
    engine.setVolume(memory.volume);
  }
  const last = memory.last;
  const index = last ? queueOf(last.archive).findIndex(tr => tr.id === last.trackId) : -1;
  if (last && index >= 0) start(last.archive, index, false, last.position);
  emit('player');
}

/** The element a video is drawn in, for Now Playing; null when nothing draws. */
export function videoView(): HTMLVideoElement | null {
  return engine.view();
}

/** Takes the video back from Now Playing without stopping it. While the floating panel has it, it stays there. */
export function parkVideo(): void {
  if (!ui.pip) engine.park();
}

/** Is what is loaded a video? Audio has nothing to float. */
export function currentIsVideo(): boolean {
  return !!ui.nowPlaying && isVideoTrack(ui.nowPlaying.track);
}

/** Into the floating panel, playing as it was. `auto`: it was not asked for, Reproduciendo was just left. */
export function showPip(auto = false): void {
  if (ui.pip || !currentIsVideo()) return;
  ui.pip = true;
  ui.pipAuto = auto;
  emit('player');
}

/** Out of the panel, playing as it was. Parked FIRST: whoever listens may take the element next. */
export function hidePip(): void {
  if (!ui.pip) return;
  ui.pip = false;
  ui.pipAuto = false;
  engine.park();
  emit('player');
}

/** The panel's ✕: nothing keeps sounding with no picture. Paused at the same second, still loaded. */
export function closePip(): void {
  if (ui.playing) setPlaying(false);
  hidePip();
  void savePosition();
}

/** "3:07", or "1:02:45" past the hour; nonsense (NaN, negative) is "0:00". */
export function formatTime(seconds: number): string {
  const s = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  const hours = Math.floor(s / 3600);
  const minutes = Math.floor((s % 3600) / 60);
  const secs = String(s % 60).padStart(2, '0');
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${secs}` : `${minutes}:${secs}`;
}

// === PRIVATE HELPERS ===

/** A new queue over the block's files on disk; false when there is nothing to play there. */
function start(blockKey: string, index: number, autoplay: boolean, resumeAt = 0): boolean {
  const found = findBlock(blockKey);
  const tracks = queueOf(blockKey);
  if (!found || !isPlayable(found.block.format) || tracks.length === 0) return false;
  // Something of this device now: what another device passed stops being read.
  if (cast) leaveCast();
  queue = new PlayerQueue(tracks, index);
  queue.repeat = ui.repeat;
  if (ui.shuffle) queue.setShuffled(true);
  currentBlock = blockKey;
  failures = 0;
  load(queue.current(), autoplay, resumeAt);
  return true;
}

function load(track: Track, autoplay: boolean, resumeAt = 0): void {
  // The notification's two buttons draw the state with every file (the service may have been restarted).
  engine.setModes?.(ui.shuffle, ui.repeat);
  const found = findBlock(currentBlock);
  ui.nowPlaying = { blockKey: currentBlock, track };
  ui.position = resumeAt;
  ui.duration = track.url ? track.seconds : 0;
  pendingSeek = resumeAt;
  engine.load(currentBlock, track, { playlist: playlistTitle(), autoplay });
  // Another playlist, now of audio: nothing to float, and it keeps sounding.
  if (!isVideoTrack(track)) hidePip();
  if (cast) {
    setPlaying(autoplay);
    return;
  }
  memory.last = {
    archive: currentBlock,
    trackId: track.id,
    playlist: found?.playlist.title ?? '',
    format: found?.block.format ?? '',
    position: resumeAt,
  };
  void persist();
  setPlaying(autoplay);
}

function stop(): void {
  if (cast) leaveCast();
  ui.pip = false;
  ui.pipAuto = false;
  engine.unload();
  queue = null;
  ui.nowPlaying = null;
  ui.playing = false;
  ui.position = 0;
  ui.duration = 0;
  emit('player');
}

function setPlaying(playing: boolean): void {
  ui.playing = playing;
  if (playing) engine.play();
  else engine.pause();
  emit('player');
}

function persist(): Promise<void> {
  const copy: PlayerMemory = { volume: memory.volume, last: memory.last ? { ...memory.last } : null };
  return api.savePlayerMemory(copy).catch(() => {});
}

/** The block's files on disk in the order the Reproductor shows them (by number unless the user sorted it). */
function queueOf(blockKey: string): Track[] {
  if (blockKey === CAST_KEY) return cast?.tracks ?? [];
  return sortTracks(onDisk(findBlock(blockKey)?.block.tracks ?? []), orderOf(blockKey));
}

function playlistTitle(): string {
  return currentBlock === CAST_KEY ? cast?.playlist ?? '' : findBlock(currentBlock)?.playlist.title ?? '';
}

function isVideoTrack(track: Track): boolean {
  if (track.url) return cast?.items.get(track.id)?.video ?? false;
  return findBlock(currentBlock)?.block.video ?? false;
}

/** Done with what another device passed: the back stops reading from it. */
function leaveCast(): void {
  cast = null;
  ui.castFrom = null;
  if (currentBlock === CAST_KEY) currentBlock = '';
  void api.connectCastClear().catch(() => {});
}
