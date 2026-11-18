// «Escuchar en», the device that PASSES: what plays here goes on sounding on another device, which reads the files
// from this one (it does not need to have them). This device goes quiet and becomes the remote control of the other
// (the same mode as «Controlar»: `connect.ts`). «Traer aquí» brings it back at the second it is at there.
//
// Kept apart from «Controlar» on purpose: controlling never changes what sounds anywhere; passing does.
// Nothing here runs on its own: no timers, nothing while nothing is passed.

import { api, type ConnectDevice } from './api';
import { notify } from './components/toasts';
import { connect, endPassControl, startPassControl } from './connect';
import { showNotice } from './overlays/notice';
import { navigate } from './router';
import { logEvent } from './diag';
import { t } from './i18n';
import * as player from './player';
import { emit, subscribe, ui } from './state';
import { errorText } from './transfer';

/** Can what plays here be passed now? (Something of THIS device plays, and it is not another device's pass.) */
export function canPass(): boolean {
  return !!connect.status?.running && !ui.remote && !ui.castFrom && player.passableQueue() !== null;
}

/** Passes what plays here to `device`. True if it sounds there now (this device is then its remote control). */
export async function passTo(device: ConnectDevice): Promise<boolean> {
  const queue = player.passableQueue();
  if (!queue) {
    notify({ tone: 'neutral', text: t('connect.cast.nothing') });
    return false;
  }
  logEvent('info', 'connect', `pass to ${device.name} #${device.id.slice(0, 6)}: ${queue.items.length} files, at ${queue.index}`);
  try {
    await api.connectCast({
      deviceId: device.id, playlist: queue.playlist, items: queue.items, index: queue.index,
      position: ui.position, playing: ui.playing, shuffle: ui.shuffle, repeat: ui.repeat,
    });
  } catch (error) {
    logEvent('warn', 'connect', `pass to ${device.name} failed: ${JSON.stringify(error)}`);
    notify({ tone: 'error', text: errorText(error) });
    return false;
  }
  ui.castTo = { deviceId: device.id, name: device.name };
  await startPassControl(device);
  emit('connect');
  return true;
}

/** «Traer aquí»: what sounds there sounds here again, from the same file and second; the other device stops. */
export async function bringHere(): Promise<void> {
  const to = ui.castTo;
  if (!to) return;
  // Where it is there right now (what the bar shows while it is controlled).
  const at = ui.nowPlaying ? { videoId: ui.nowPlaying.track.id, position: ui.position, playing: ui.playing } : null;
  logEvent('info', 'connect', `bring back from ${to.name}${at ? ` at ${at.videoId} ${Math.round(at.position)}s` : ''}`);
  await finish('bring');
  if (at && player.playTrackById(at.videoId, at.position)) {
    if (!at.playing) player.togglePlay();
  }
}

/** «Detener»: the other device stops and nothing comes back here (this device stays paused where it was). */
export async function stopPass(): Promise<void> {
  if (!ui.castTo) return;
  logEvent('info', 'connect', `stop the pass to ${ui.castTo.name}`);
  await finish('stop');
}

/** The pass is over for whatever reason (also when this device plays something of its own, or the other one goes). */
export function passEnded(reason: string): void {
  if (!ui.castTo) return;
  ui.castTo = null;
  void api.connectCastEnd(reason).catch(() => {});
  emit('connect');
}

async function finish(reason: string): Promise<void> {
  ui.castTo = null;
  await api.connectCastEnd(reason).catch(error => notify({ tone: 'error', text: errorText(error) }));
  await endPassControl();
  emit('connect');
}

// === THE DEVICE THAT RECEIVES ===

/** «Escuchar en este dispositivo»: asks first (unless the user turned the notice off), then what comes from the other device stops. */
export function stopListening(): void {
  const from = ui.castFrom;
  if (!from) return;
  showNotice({
    id: 'stopListening',
    title: t('connect.listen.stop.title'),
    header: t('connect.listen.stop.header', from),
    body: t('connect.listen.stop.body', from),
    confirm: t('connect.listen.stop.confirm'),
    run: () => {
      logEvent('info', 'connect', `stop listening to ${from}`);
      player.stopHere();
      navigate('player');
    },
  });
}

/** «Escuchar aquí»: asks `device` to pass what it plays; when it arrives, the view of what is received opens. */
export async function listenFrom(device: ConnectDevice): Promise<void> {
  if (!device.playing) {
    notify({ tone: 'neutral', text: t('connect.listen.idle', device.name) });
    return;
  }
  logEvent('info', 'connect', `listen from ${device.name} #${device.id.slice(0, 6)}`);
  try {
    await api.connectPassRequest(device.id);
  } catch (error) {
    notify({ tone: 'error', text: errorText(error) });
    return;
  }
  if (await arrives()) navigate('cast-queue');
  else notify({ tone: 'warning', text: t('connect.listen.timeout', device.name) });
}

/** True when something is passed here (it answers within a few seconds), false if it never comes. */
function arrives(): Promise<boolean> {
  if (ui.castFrom) return Promise.resolve(true);
  return new Promise(resolve => {
    const done = (value: boolean) => {
      window.clearTimeout(timer);
      stop();
      resolve(value);
    };
    const stop = subscribe('player', () => { if (ui.castFrom) done(true); });
    const timer = window.setTimeout(() => done(false), 8000);
  });
}

// === THE DEVICE THAT PASSES, ASKED OR TOLD ===

/** A paired device asked to listen to what plays here («Escuchar aquí»): it is passed, no questions. */
export async function passToRequester(deviceId: string): Promise<void> {
  const device = connect.devices.find(other => other.id === deviceId);
  if (!device) return;
  if (!canPass()) {
    notify({ tone: 'neutral', text: t('connect.listen.nothingHere', device.name) });
    return;
  }
  await passTo(device);
}

/** The device this one passed its music to stopped listening: this device goes back to its own player (paused where it was). */
export async function receiverLeft(): Promise<void> {
  if (!ui.castTo) return;
  logEvent('info', 'connect', `${ui.castTo.name} stopped listening`);
  ui.castTo = null;
  await endPassControl();
  emit('connect');
}
