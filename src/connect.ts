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

import { api, type ConnectDevice, type ConnectPlayerState, type ConnectStatus } from './api';
import { notify } from './components/toasts';
import { actionsFor, projectPosition, remoteTrack, shouldPublish, toWireState } from './connectLogic';
import { t } from './i18n';
import { findBlock } from './library';
import * as player from './player';
import { emit, subscribe, ui } from './state';
import { errorText } from './transfer';

export const connect = {
  status: null as ConnectStatus | null,
  devices: [] as ConnectDevice[],
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
  emit('connect');
}

export function startConnect(): void {
  api.onConnectDevices(() => void loadConnect());
  api.onConnectRemoteState(onRemoteState);
  api.onConnectRemoteClosed(onRemoteClosed);
  api.onConnectCommand(incoming => onCommand(incoming.command));
  subscribe('player', () => publishIfWorth());
  document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && void loadConnect());
  void loadConnect();
}

// === BEING CONTROLLED ===

let lastPublished: ConnectPlayerState | null = null;
let lastPublishedAt = 0;

/** This player's state, said to whoever controls it, when it changed in a way that matters. */
function publishIfWorth(force = false): void {
  if (!connect.status?.running || ui.remote) return;
  const state = toWireState(ui, blockKey => {
    const found = findBlock(blockKey);
    return found ? { playlist: found.playlist.title, format: found.block.format, video: found.block.video } : null;
  });
  const elapsed = (Date.now() - lastPublishedAt) / 1000;
  if (!force && !shouldPublish(lastPublished, state, elapsed)) return;
  lastPublished = state;
  lastPublishedAt = Date.now();
  api.connectPublish(state);
}

/** What a controller asked: done with the functions the buttons use. Ignored while this device is a controller itself. */
function onCommand(command: Parameters<typeof actionsFor>[0]): void {
  if (ui.remote) return;
  for (const action of actionsFor(command, ui)) {
    switch (action.kind) {
      case 'toggle': player.togglePlay(); break;
      case 'next': player.next(); break;
      case 'previous': player.previous(); break;
      case 'seek': player.seek(action.seconds); break;
      case 'volume': player.setVolume(action.volume); player.saveVolume(); break;
      case 'shuffle': player.toggleShuffle(); break;
      case 'repeat': for (let press = 0; press < action.times; press++) player.cycleRepeat(); break;
      case 'load':
        if (!player.playVideoId(action.videoId, action.position, action.autoplay, action.playlist)) {
          notify({ tone: 'warning', text: t('connect.cannotLoad') });
        }
        break;
    }
  }
  publishIfWorth(true);
}

// === CONTROLLING ===

let snapshot: player.LocalSnapshot | null = null;
let remoteState: ConnectPlayerState | null = null;
let receivedAt = 0;
let ticker: number | undefined;

const route = (command: Parameters<typeof actionsFor>[0]): void => {
  api.connectSend(command).catch(error => notify({ tone: 'error', text: errorText(error) }));
};

/** Starts controlling a paired device: from now on the player shows and drives it. */
export async function controlDevice(device: ConnectDevice): Promise<boolean> {
  if (ui.remote?.deviceId === device.id) return true;
  try {
    await api.connectSelect(device.id);
  } catch (error) {
    notify({ tone: 'error', text: errorText(error) });
    void loadConnect();
    return false;
  }
  if (!ui.remote) snapshot = player.enterRemote(route);
  ui.remote = { deviceId: device.id, name: device.name };
  remoteState = null;
  window.clearInterval(ticker);
  ticker = window.setInterval(moveClock, 500);
  void loadConnect();
  emit('player');
  return true;
}

/** Back to this device. */
export async function stopControlling(): Promise<void> {
  await api.connectDeselect().catch(() => {});
  leave();
}

function leave(): void {
  window.clearInterval(ticker);
  ticker = undefined;
  remoteState = null;
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
  ui.volume = state.volume;
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

function onRemoteClosed(deviceId: string): void {
  if (ui.remote?.deviceId !== deviceId) return;
  const name = ui.remote.name;
  leave();
  notify({ tone: 'warning', text: t('connect.lost', name) });
}

// === HANDING THE PLAYBACK OVER ===

/** «Pasar aquí → allí»: the playlist that sounds here goes on in `device`, from the same second. */
export async function transferTo(device: ConnectDevice): Promise<void> {
  const queue = player.currentQueue();
  if (!queue || !ui.nowPlaying) {
    notify({ tone: 'warning', text: t('connect.nothingToHand') });
    return;
  }
  const position = ui.position;
  if (!await controlDevice(device)) return;
  route({ cmd: 'load', playlist: queue.playlist, queue: queue.items, index: queue.index, position, autoplay: true });
}

/** «Traer aquí»: what sounds on the other device goes on here, if this device has the file. */
export async function bringHere(): Promise<void> {
  const state = remoteState;
  if (!ui.remote || !state?.nowPlaying) {
    await stopControlling();
    return;
  }
  const { videoId, playlist } = state.nowPlaying;
  const [position, playing] = [projectPosition(state, (Date.now() - receivedAt) / 1000), state.playing];
  route({ cmd: 'pause' });
  await stopControlling();
  if (!player.playVideoId(videoId, position, playing, playlist)) notify({ tone: 'warning', text: t('connect.notHere') });
}
