// Soundify Connect on the page: which devices there are, controlling one of them, and
// being controlled. The pure decisions are in `connectLogic.ts`; the back (`connect.rs`)
// does the network. See `PLAN-CONNECT.md`.
//
// Controlling a device = its state is put in `ui` (so the bar, Reproduciendo and the
// notifications show IT) and the player's actions go to it (`player.enterRemote`). The
// local player is paused and comes back as it was when control ends.
//
// Being controlled = this player's state is published whenever it is worth it, and what
// the controller asks is done with the same functions the buttons call.

import { api, type ConnectLibraryEntry, type ConnectDevice, type ConnectPlayerState, type ConnectQueue, type ConnectStatus } from './api';
import { notify } from './components/toasts';
import { count, logEvent } from './diag';
import { VOLUME_ECHO_MS, actionsFor, describeCommand, projectPosition, remoteTrack, shouldPublish, toWireState, volumeToShow } from './connectLogic';
import { t } from './i18n';
import { findBlock, library as libraryOf, onDisk } from './library';
import * as player from './player';
import { emit, subscribe, ui } from './state';
import { errorText } from './transfer';
import { passEnded } from './cast';

export const connect = {
  status: null as ConnectStatus | null,
  devices: [] as ConnectDevice[],
  /** The playlist the device being controlled is playing: its «remote playlist» (null until it says). */
  queue: null as ConnectQueue | null,
  /** The playlists of the library of the device being controlled: what to pick from when nothing plays there. */
  library: [] as ConnectLibraryEntry[],
  /** The playlist of that library that is being browsed (its files arrive after asking; `queue` null = still loading). */
  browsed: null as { key: string; title: string; queue: ConnectQueue | null } | null,
  /** Show the library even though the device plays a playlist (to pick another). */
  pickingLibrary: false,
};

/** The device list and who is connected, as the back has them now. */
export async function loadConnect(): Promise<void> {
  count('loadConnect');
  const wasRunning = !!connect.status?.running;
  try {
    const [status, devices] = await Promise.all([api.connectStatus(), api.connectDevices()]);
    connect.status = status;
    connect.devices = devices;
  } catch {
    return;
  }
  if (!wasRunning && connect.status?.running) publishLibrarySoon();
  // Turned on just now: say what is playing. Only then: every state that arrives from another device refreshes this list
  // too, and a forced publish here made each device answer each state with its own, round and round. With three devices
  // that grew until a controller's queue overflowed and it was dropped, over and over. A controller that connects later is
  // sent the last state by the back itself, and changes are published by `subscribe('player')`.
  // What another device is playing is shown only in the Devices window («Sonando: …»), never as a notice: one per song
  // flooded the screen and the notice list.
  publishIfWorth(!wasRunning && !!connect.status?.running);
  emit('connect');
}

export function startConnect(): void {
  // A device that floods this one with states would make the list reload a hundred times a second (the log counted ~4000 per 30 s):
  // one reload per quarter of a second is as fresh as anybody can see.
  let refreshing: number | undefined;
  api.onConnectDevices(() => {
    if (refreshing !== undefined) return;
    refreshing = window.setTimeout(() => {
      refreshing = undefined;
      void loadConnect();
    }, 250);
  });
  api.onConnectRemoteState(onRemoteState);
  api.onConnectRemoteQueue(onRemoteQueue);
  api.onConnectRemoteLibrary(onRemoteLibrary);
  api.onConnectRemoteBrowse(onRemoteBrowse);
  // This device's own library, for whoever controls it: said when it changes (and the back keeps it for whoever connects).
  subscribe('library', publishLibrarySoon);
  api.onConnectRemoteClosed(onRemoteClosed);
  api.onConnectCommand(incoming => onCommand(incoming.command, incoming.from, incoming.castBase));
  player.onCastLost(from => {
    logEvent('warn', 'connect', `cast from ${from}: its files stopped answering`);
    notify({ tone: 'error', text: t('connect.castLost', from) });
  });
  subscribe('player', () => publishIfWorth());
  document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && void loadConnect());
  void loadConnect();
}

// === BEING CONTROLLED ===

let lastPublished: ConnectPlayerState | null = null;
let lastPublishedAt = 0;
/** The playlist whose files were last published as the queue: they only go again when it changes. */
let publishedBlock: string | null | undefined;

const blockInfo = (blockKey: string) => {
  if (blockKey === player.CAST_KEY) return player.castBlockInfo();
  const found = findBlock(blockKey);
  return found ? { playlist: found.playlist.title, format: found.block.format, video: found.block.video } : null;
};

/**
 * This device starts controlling another: what it said last (maybe «playing») is no longer true, because the local player was
 * paused. Whoever controls THIS device from now on must see it as it is.
 */
function publishPaused(local: player.LocalSnapshot): void {
  if (!connect.status?.running) return;
  const state = toWireState({ ...local, playing: false }, blockInfo);
  lastPublished = state;
  lastPublishedAt = Date.now();
  count('publish');
  api.connectPublish(state);
}

/** This player's state, said to whoever controls it, when it changed in a way that matters. */
function publishIfWorth(force = false): void {
  if (!connect.status?.running || ui.remote) return;
  const state = toWireState(ui, blockInfo);
  const elapsed = (Date.now() - lastPublishedAt) / 1000;
  if (!force && !shouldPublish(lastPublished, state, elapsed)) return;
  lastPublished = state;
  lastPublishedAt = Date.now();
  count(force ? 'publishForced' : 'publish');
  api.connectPublish(state);
  const blockKey = ui.nowPlaying?.blockKey ?? null;
  if (force || blockKey !== publishedBlock) {
    publishedBlock = blockKey;
    const queue = player.currentQueue();
    api.connectPublishQueue(queue ? { playlist: queue.playlist, items: queue.items } : null);
  }
}

/**
 * What a controller asked: done with the functions the buttons use. A device is either a controller or a player at a time, and
 * the last to ask wins: if this one was controlling another device, it stops (back to its own player) and does what it is asked.
 */
function onCommand(command: Parameters<typeof actionsFor>[0], from = '', castBase?: string): void {
  count('commandIn');
  // A slider dragged sends dozens: they are counted, not written.
  if (command.cmd !== 'volume' && command.cmd !== 'seek') logEvent('info', 'connect', `command ${command.cmd} from ${from || '?'}${ui.remote ? ` (this device was controlling ${ui.remote.name}: it stops)` : ''}`);
  // Listing a playlist for a controller changes nothing here: this device goes on controlling another one, if it did.
  if (ui.remote && command.cmd !== 'browse') {
    void api.connectDeselect().catch(() => {});
    leave();
  }
  sayWhoDid(command, from);
  // «Escuchar en»: what the other device plays sounds here now, read from it (or stops, when it takes it back).
  if (command.cmd === 'cast') {
    if (!player.playCast(from || t('connect.someone'), command, castBase)) logEvent('warn', 'connect', `cast from ${from || '?'} refused: empty or bad queue`);
  } else if (command.cmd === 'castEnd') {
    player.endCast();
  }
  for (const action of actionsFor(command, ui)) {
    switch (action.kind) {
      case 'toggle': player.togglePlay(); break;
      case 'next': player.next(); break;
      case 'previous': player.previous(); break;
      case 'seek': player.seek(action.seconds); break;
      case 'volume': player.setVolume(action.volume); player.saveVolume(); break;
      case 'shuffle': player.toggleShuffle(); break;
      case 'repeat': for (let press = 0; press < action.times; press++) player.cycleRepeat(); break;
      case 'play': player.playTrackById(action.videoId); break;
      case 'browse': api.connectPublishBrowse(action.key, player.browseQueue(action.key)); break;
      case 'playFrom': player.playFromBlock(action.key, action.videoId); break;
    }
  }
  publishIfWorth(true);
}

/** This device's library, as a controller lists it: every playlist in every format with files on disk (no files, just the summary). */
function libraryEntries(): ConnectLibraryEntry[] {
  const entries: ConnectLibraryEntry[] = [];
  for (const playlist of libraryOf.playlists) {
    for (const block of playlist.formats) {
      const count = onDisk(block.tracks).length;
      if (count > 0 && player.isPlayable(block.format)) entries.push({ key: block.key, title: playlist.title, format: block.format, video: block.video, count });
    }
  }
  return entries;
}

let libraryTimer: number | undefined;
/** Said a moment after the library changed (a download touches it many times), and only while Connect is on. */
function publishLibrarySoon(): void {
  if (libraryTimer !== undefined || !connect.status?.running) return;
  libraryTimer = window.setTimeout(() => {
    libraryTimer = undefined;
    if (connect.status?.running) api.connectPublishLibrary(libraryEntries());
  }, 800);
}

/** The same notice is not repeated while a slider is dragged or a button pressed again and again. */
let lastSaid = { key: '', at: 0 };

/** «Móvil controla ahora este dispositivo»: the device that is controlled says who took it (and nothing of what it then does). */
function sayWhoDid(command: Parameters<typeof actionsFor>[0], from: string): void {
  const said = describeCommand(command, from || t('connect.someone'));
  if (!said) return;
  const now = Date.now();
  if (lastSaid.key === said.key && now - lastSaid.at < 3000) return;
  lastSaid = { key: said.key, at: now };
  notify({ tone: 'neutral', text: t(said.key, ...said.args) });
}

// === CONTROLLING ===

let snapshot: player.LocalSnapshot | null = null;
let remoteState: ConnectPlayerState | null = null;
let receivedAt = 0;
let ticker: number | undefined;

let volumeSentAt = 0;
let volumeSettle: number | undefined;

const route = (command: Parameters<typeof actionsFor>[0]): void => {
  count('commandOut');
  if (command.cmd === 'volume') volumeSentAt = Date.now();
  api.connectSend(command).catch(error => notify({ tone: 'error', text: errorText(error) }));
};

/** Starts controlling a paired device: from now on the player shows and drives it. */
export async function controlDevice(device: ConnectDevice): Promise<boolean> {
  if (ui.remote?.deviceId === device.id) return true;
  logEvent('info', 'connect', `control ${device.name} #${device.id.slice(0, 6)} (ready=${device.ready}, playing=${device.playing})`);
  let current: ConnectPlayerState | null;
  try {
    current = await api.connectSelect(device.id);
  } catch (error) {
    logEvent('warn', 'connect', `could not control ${device.name}: ${JSON.stringify(error)}`);
    notify({ tone: 'error', text: errorText(error) });
    void loadConnect();
    return false;
  }
  await beginControl(device, current);
  return true;
}

/** «Escuchar en»: what plays here was passed to `device` (the back already controls it): this device becomes its remote. */
export async function startPassControl(device: ConnectDevice): Promise<void> {
  if (ui.remote?.deviceId === device.id) return;
  await beginControl(device, null);
}

/** The pass ended: back to this device's own player (paused where it was). */
export async function endPassControl(): Promise<void> {
  await api.connectDeselect().catch(() => {});
  leave();
}

async function beginControl(device: ConnectDevice, current: ConnectPlayerState | null): Promise<void> {
  if (!ui.remote) {
    snapshot = player.enterRemote(route, playHere);
    publishPaused(snapshot);
  }
  ui.remote = { deviceId: device.id, name: device.name };
  window.clearInterval(ticker);
  ticker = window.setInterval(moveClock, 500);
  // What it is doing right now, at once: it is already connected and has been saying it all along.
  remoteState = current;
  receivedAt = Date.now();
  connect.queue = await api.connectQueueOf(device.id).catch(() => null);
  connect.library = await api.connectLibraryOf(device.id).catch(() => []);
  connect.browsed = null;
  connect.pickingLibrary = false;
  if (current) show(current, current.position);
  else emit('player');
  void loadConnect();
}

/** The remote playlist: asks the device to play that file of it. Playing the one that sounds pauses or resumes it. */
export function playRemoteTrack(videoId: string): void {
  if (ui.nowPlaying?.track.id === videoId) player.togglePlay();
  else route({ cmd: 'playTrack', videoId });
}

/** Back to this device. */
export async function stopControlling(): Promise<void> {
  logEvent('info', 'connect', `stop controlling ${ui.remote?.name ?? '-'} (the user chose this device)`);
  await api.connectDeselect().catch(() => {});
  leave();
}

/**
 * A file of THIS device was played while another was controlled: control is let go of (not the connection: it stays,
 * and the notice offers to take the other back) and this device plays it. The other keeps playing what it was.
 */
function playHere(): void {
  const was = ui.remote;
  if (!was) return;
  logEvent('info', 'connect', `playing here: control of ${was.name} let go of, still connected`);
  void api.connectDeselect().catch(() => {});
  leave();
  notify({
    tone: 'neutral',
    text: t('connect.playingHere', was.name),
    action: { label: t('connect.controlAgain'), run: () => {
      const device = connect.devices.find(d => d.id === was.deviceId);
      if (device) void controlDevice(device);
    } },
  });
}

function leave(): void {
  // Leaving the control of a device this one passed its music to ends the pass too (the other one stops).
  passEnded('left');
  window.clearInterval(ticker);
  ticker = undefined;
  window.clearTimeout(volumeSettle);
  volumeSentAt = 0;
  remoteState = null;
  connect.queue = null;
  connect.library = [];
  connect.browsed = null;
  connect.pickingLibrary = false;
  if (snapshot) {
    ui.remote = null;
    player.leaveRemote(snapshot);
    snapshot = null;
  }
  publishIfWorth(true);
  void loadConnect();
}

function onRemoteState(deviceId: string, state: ConnectPlayerState): void {
  if (ui.remote?.deviceId !== deviceId) return;
  remoteState = state;
  receivedAt = Date.now();
  show(state, state.position);
}

/** The other device's state, in the fields every screen already reads. */
function show(state: ConnectPlayerState, position: number): void {
  ui.nowPlaying = state.nowPlaying ? { blockKey: '', track: remoteTrack(state.nowPlaying.videoId, state.nowPlaying.title, state.duration) } : null;
  ui.playing = state.playing;
  ui.position = position;
  ui.duration = state.duration;
  ui.volume = volumeToShow(state.volume, ui.volume, Date.now() - volumeSentAt);
  if (ui.volume !== state.volume) {
    // Its answer was held back: once the window is over, what it really has.
    window.clearTimeout(volumeSettle);
    volumeSettle = window.setTimeout(() => {
      if (!ui.remote || !remoteState || ui.volume === remoteState.volume) return;
      ui.volume = remoteState.volume;
      emit('player');
    }, VOLUME_ECHO_MS);
  }
  ui.shuffle = state.shuffle;
  ui.repeat = state.repeat;
  emit('player');
}

/** The other device only says what it does when something changes: the clock moves on from the last word. */
function moveClock(): void {
  if (!ui.remote || !remoteState?.playing) return;
  ui.position = projectPosition(remoteState, (Date.now() - receivedAt) / 1000);
  emit('player');
}

function onRemoteQueue(deviceId: string, queue: ConnectQueue | null): void {
  if (ui.remote?.deviceId !== deviceId) return;
  connect.queue = queue;
  // It plays a playlist now (the one just picked, or another): that is what is shown, not the list to pick from.
  if (queue) {
    connect.browsed = null;
    connect.pickingLibrary = false;
  }
  emit('connect');
}

function onRemoteLibrary(deviceId: string, playlists: ConnectLibraryEntry[]): void {
  if (ui.remote?.deviceId !== deviceId) return;
  connect.library = playlists;
  emit('connect');
}

function onRemoteBrowse(deviceId: string, key: string, queue: ConnectQueue | null): void {
  if (ui.remote?.deviceId !== deviceId || connect.browsed?.key !== key) return;
  connect.browsed = { ...connect.browsed, queue: queue ?? { playlist: connect.browsed.title, items: [] } };
  emit('connect');
}

/** Lists the files of a playlist of the device being controlled (it plays nothing for that). */
export function browseRemote(entry: ConnectLibraryEntry): void {
  if (!ui.remote) return;
  connect.browsed = { key: entry.key, title: entry.title, queue: null };
  connect.pickingLibrary = false;
  emit('connect');
  route({ cmd: 'browse', key: entry.key });
}

/** Plays a file of the playlist being browsed, on the device being controlled. */
export function playRemoteFrom(key: string, videoId: string): void {
  route({ cmd: 'playFrom', key, videoId });
}

/** Back to the list of playlists of the device being controlled (or to the one it plays). */
export function showRemoteLibrary(show: boolean): void {
  connect.browsed = null;
  connect.pickingLibrary = show;
  emit('connect');
}

function onRemoteClosed(deviceId: string): void {
  if (ui.remote?.deviceId !== deviceId) return;
  const name = ui.remote.name;
  logEvent('warn', 'connect', `lost the connection with ${name} #${deviceId.slice(0, 6)} while controlling it`);
  leave();
  notify({ tone: 'warning', text: t('connect.lost', name) });
}
