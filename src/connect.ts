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

import { api, type ConnectDevice, type ConnectPlayerState, type ConnectQueue, type ConnectStatus } from './api';
import { notify } from './components/toasts';
import { VOLUME_ECHO_MS, actionsFor, describeCommand, projectPosition, remoteTrack, shouldPublish, toWireState, volumeToShow } from './connectLogic';
import { t } from './i18n';
import { findBlock } from './library';
import * as player from './player';
import { emit, subscribe, ui } from './state';
import { errorText } from './transfer';

export const connect = {
  status: null as ConnectStatus | null,
  devices: [] as ConnectDevice[],
  /** The playlist the device being controlled is playing: its «remote playlist» (null until it says). */
  queue: null as ConnectQueue | null,
};

/** The device list and who is connected, as the back has them now. */
export async function loadConnect(): Promise<void> {
  try {
    const [status, devices] = await Promise.all([api.connectStatus(), api.connectDevices()]);
    connect.status = status;
    connect.devices = devices;
  } catch {
    return;
  }
  // Turned on just now (or a controller is waiting for the first state): say what is playing.
  publishIfWorth(true);
  announcePlaying();
  emit('connect');
}

// === ANOTHER DEVICE STARTS PLAYING ===

/** Which file each device was last announced playing, so it is said once and not at every refresh. */
const announced = new Map<string, string>();

/**
 * Like Spotify noticing you listen somewhere else: when a device starts playing and this one is not
 * playing or controlling anything, a notice offers to control it with one tap.
 */
function announcePlaying(): void {
  for (const device of connect.devices) {
    const title = device.playing ? device.nowPlaying?.title : undefined;
    if (!title) {
      announced.delete(device.id);
      continue;
    }
    if (announced.get(device.id) === title) continue;
    announced.set(device.id, title);
    if (ui.remote || ui.playing || !device.ready) continue;
    notify({
      tone: 'neutral',
      text: t('connect.autoDetected', device.name, title),
      action: { label: t('connect.control'), run: () => void controlDevice(device) },
    });
  }
}

export function startConnect(): void {
  api.onConnectDevices(() => void loadConnect());
  api.onConnectRemoteState(onRemoteState);
  api.onConnectRemoteQueue(onRemoteQueue);
  api.onConnectRemoteClosed(onRemoteClosed);
  api.onConnectCommand(incoming => onCommand(incoming.command, incoming.from));
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
function onCommand(command: Parameters<typeof actionsFor>[0], from = ''): void {
  if (ui.remote) {
    void api.connectDeselect().catch(() => {});
    leave();
  }
  sayWhoDid(command, from);
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
    }
  }
  publishIfWorth(true);
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
  if (command.cmd === 'volume') volumeSentAt = Date.now();
  api.connectSend(command).catch(error => notify({ tone: 'error', text: errorText(error) }));
};

/** Starts controlling a paired device: from now on the player shows and drives it. */
export async function controlDevice(device: ConnectDevice): Promise<boolean> {
  if (ui.remote?.deviceId === device.id) return true;
  let current: ConnectPlayerState | null;
  try {
    current = await api.connectSelect(device.id);
  } catch (error) {
    notify({ tone: 'error', text: errorText(error) });
    void loadConnect();
    return false;
  }
  if (!ui.remote) {
    snapshot = player.enterRemote(route);
    publishPaused(snapshot);
  }
  ui.remote = { deviceId: device.id, name: device.name };
  window.clearInterval(ticker);
  ticker = window.setInterval(moveClock, 500);
  // What it is doing right now, at once: it is already connected and has been saying it all along.
  remoteState = current;
  receivedAt = Date.now();
  connect.queue = await api.connectQueueOf(device.id).catch(() => null);
  if (current) show(current, current.position);
  else emit('player');
  void loadConnect();
  return true;
}

/** The remote playlist: asks the device to play that file of it. Playing the one that sounds pauses or resumes it. */
export function playRemoteTrack(videoId: string): void {
  if (ui.nowPlaying?.track.id === videoId) player.togglePlay();
  else route({ cmd: 'playTrack', videoId });
}

/** Back to this device. */
export async function stopControlling(): Promise<void> {
  await api.connectDeselect().catch(() => {});
  leave();
}

function leave(): void {
  window.clearInterval(ticker);
  ticker = undefined;
  window.clearTimeout(volumeSettle);
  volumeSentAt = 0;
  remoteState = null;
  connect.queue = null;
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
  emit('connect');
}

function onRemoteClosed(deviceId: string): void {
  if (ui.remote?.deviceId !== deviceId) return;
  const name = ui.remote.name;
  leave();
  notify({ tone: 'warning', text: t('connect.lost', name) });
}
