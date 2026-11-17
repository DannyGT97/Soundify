// «Escuchar en», the device that PASSES: what plays here goes on sounding on another device, which reads the files
// from this one (it does not need to have them). This device goes quiet and becomes the remote control of the other
// (the same mode as «Controlar»: `connect.ts`). «Traer aquí» brings it back at the second it is at there.
//
// Kept apart from «Controlar» on purpose: controlling never changes what sounds anywhere; passing does.
// Nothing here runs on its own: no timers, nothing while nothing is passed.

import { api, type ConnectDevice } from './api';
import { notify } from './components/toasts';
import { connect, endPassControl, startPassControl } from './connect';
import { logEvent } from './diag';
import { t } from './i18n';
import * as player from './player';
import { emit, ui } from './state';
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
