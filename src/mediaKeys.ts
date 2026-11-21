// The media keys of the system (Windows: the keyboard's play/pause/next/previous, a headset's button, the volume flyout's player).
// Two ways: what a key asks is done with the same functions the bar's buttons use (so it also works while another device is
// controlled or passes its music here), and what plays here is told to the system so it shows it, with its state, shuffle
// and repeat (`media_keys.rs`). Android has its own (the notification of the player), the browser has none.

import { api } from './api';
import type { MediaKey, MediaSessionUpdate } from './api/types';
import { t } from './i18n';
import { onAndroid } from './platform';
import * as player from './player';
import { subscribe, ui } from './state';

/** What the system should show now. */
export function currentSession(): MediaSessionUpdate {
  const track = ui.nowPlaying?.track;
  return {
    active: !!track,
    title: track?.title ?? '',
    subtitle: ui.remote ? ui.remote.name : player.currentQueue()?.playlist ?? '',
    playing: ui.playing,
    shuffle: ui.shuffle,
    repeat: ui.repeat,
    tips: {
      previous: t('player.previous'), play: t('player.play'), pause: t('player.pause'), next: t('player.next'),
      shuffle: t('player.shuffle'), repeat: t(`player.repeat.${ui.repeat}`),
    },
  };
}

/** What a key asked for, done (the bar's functions: the same thing a click would do). */
export function pressKey(key: MediaKey): void {
  switch (key.action) {
    case 'play':
      if (!ui.playing && ui.nowPlaying) player.togglePlay();
      break;
    case 'pause':
    case 'stop':
      if (ui.playing) player.togglePlay();
      break;
    case 'next':
      player.next();
      break;
    case 'previous':
      player.previous();
      break;
    case 'shuffle':
      if (ui.shuffle !== (key.value === 'true')) player.toggleShuffle();
      break;
    case 'repeat': {
      const wanted = key.value === 'one' || key.value === 'all' ? key.value : 'off';
      // Another device's repeat does not change at once: one step is asked, not three.
      for (let step = 0; step < (ui.remote ? 1 : 3) && ui.repeat !== wanted; step += 1) player.cycleRepeat();
      break;
    }
  }
}

export function startMediaKeys(): void {
  if (onAndroid) return;
  api.onMediaKey(pressKey);
  let said = '';
  const publish = () => {
    const session = currentSession();
    // The player ticks several times a second: only what changed is told.
    const signature = `${session.active}|${session.title}|${session.subtitle}|${session.playing}|${session.shuffle}|${session.repeat}|${session.tips.play}`;
    if (signature === said) return;
    said = signature;
    api.mediaSession(session);
  };
  subscribe('player', publish);
  subscribe('connect', publish);
  publish();
}
