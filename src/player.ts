// The player's actions (the Java `PlayerService`): one player for the whole
// app, so it keeps sounding while moving between screens. What to play next
// is the `PlayerQueue`'s; what actually sounds is the PlayerBackend behind
// `api` (the webview's media element in the app, a clock in the mock).
//
// The last file and its second are kept between runs (`PlayerMemory`): at
// startup it comes back to the bar PAUSED where it was left.

import { api, playerBackend as engine, type PlayerMemory, type Track } from './api';
import { findBlock, onDisk } from './library';
import { PlayerQueue, nextRepeat } from './playerQueue';
import { emit, ui } from './state';

/**
 * What the built-in player can open, by format NAME (ALAC is .m4a too and
 * does not play). Tried one by one on WebView2: AVI and FLV do not open in
 * any codec yt-dlp leaves them in, and they are not converted (decided: the
 * rest of the list is listed but dimmed, with the reason).
 */
export const PLAYABLE: ReadonlySet<string> = new Set([
  'MP3', 'OPUS', 'M4A', 'FLAC', 'AAC', 'VORBIS', 'WAV', 'MP4', 'MKV', 'MOV', 'WEBM',
]);

let queue: PlayerQueue<Track> | null = null;
let currentBlock = '';
// Files that failed one after another: when all of them have, it stops
// instead of going round the list for ever.
let failures = 0;
// The second to jump to once the file is open; before that a seek is ignored.
let pendingSeek = 0;
let memory: PlayerMemory = { volume: ui.volume, last: null };

engine.onTime(seconds => {
  ui.position = seconds;
  emit('player');
});
engine.onDuration(seconds => {
  failures = 0;
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
  failures++;
  if (!queue || failures >= queue.size) stop();
  else load(queue.next(), true);
});

// === PUBLIC API ===

export function isPlayable(format: string | null | undefined): boolean {
  return !!format && PLAYABLE.has(format.toUpperCase());
}

/** Plays `track` of a block, with the block's files on disk as the queue. */
export function play(blockKey: string, track: Track): void {
  const index = queueOf(blockKey).findIndex(tr => tr.id === track.id);
  if (index >= 0) start(blockKey, index, true);
}

/** From the first file (shuffled, the rest come in a fixed random order). */
export function playAll(blockKey: string): void {
  start(blockKey, 0, true);
}

export function togglePlay(): void {
  if (!ui.nowPlaying) return;
  setPlaying(!ui.playing);
  if (!ui.playing) void savePosition();
}

export function next(): void {
  if (!queue) return;
  failures = 0;
  load(queue.next(), true);
}

export function previous(): void {
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
  ui.position = seconds;
  engine.seek(seconds);
  emit('player');
}

export function toggleShuffle(): void {
  ui.shuffle = !ui.shuffle;
  queue?.setShuffled(ui.shuffle);
  emit('player');
}

export function cycleRepeat(): void {
  ui.repeat = nextRepeat(ui.repeat);
  if (queue) queue.repeat = ui.repeat;
  emit('player');
}

/** While the slider moves; `saveVolume` once it is let go. */
export function setVolume(volume: number): void {
  ui.volume = volume;
  engine.setVolume(volume);
  emit('player');
}

export function saveVolume(): void {
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
  if (memory.last && ui.nowPlaying) memory.last.position = ui.position;
  return persist();
}

/**
 * At startup: the saved volume, and the last file back in the bar, paused at
 * its second. If it is not on disk any more there is simply no bar.
 */
export async function restoreLast(): Promise<void> {
  memory = await api.playerMemory().catch(() => memory);
  ui.volume = memory.volume;
  engine.setVolume(memory.volume);
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
  return !!ui.nowPlaying && (findBlock(ui.nowPlaying.blockKey)?.block.video ?? false);
}

/** Into the floating panel, playing as it was. */
export function showPip(): void {
  if (ui.pip || !currentIsVideo()) return;
  ui.pip = true;
  emit('player');
}

/** Out of the panel, playing as it was. Parked FIRST: whoever listens may take the element next. */
export function hidePip(): void {
  if (!ui.pip) return;
  ui.pip = false;
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
  queue = new PlayerQueue(tracks, index);
  queue.repeat = ui.repeat;
  if (ui.shuffle) queue.setShuffled(true);
  currentBlock = blockKey;
  failures = 0;
  load(queue.current(), autoplay, resumeAt);
  return true;
}

function load(track: Track, autoplay: boolean, resumeAt = 0): void {
  const found = findBlock(currentBlock);
  ui.nowPlaying = { blockKey: currentBlock, track };
  ui.position = resumeAt;
  ui.duration = 0;
  pendingSeek = resumeAt;
  engine.load(currentBlock, track);
  // Another playlist, now of audio: nothing to float, and it keeps sounding.
  if (!found?.block.video) hidePip();
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
  ui.pip = false;
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

function queueOf(blockKey: string): Track[] {
  return onDisk(findBlock(blockKey)?.block.tracks ?? []);
}
