// The player's controls (shuffle, previous, play, next, repeat) and its
// progress line. The bottom bar carries them, and so does Reproduciendo when
// the bar is not shown (only the side menu's mini player) or in full screen:
// a screen that plays something and offers no way to drive it was the complaint.

import { h } from '../dom';
import { t } from '../i18n';
import { icon, type IconName } from '../icons';
import * as player from '../player';
import { subscribeWhile, ui } from '../state';

export function buildTransport(): HTMLElement {
  const shuffle = control('shuffle', 'player.shuffle', player.toggleShuffle);
  const previous = control('previous', 'player.previous', player.previous, true);
  const play = h('button', { class: 'player-play', onclick: player.togglePlay });
  const next = control('next', 'player.next', player.next, true);
  const repeat = control('repeat', 'player.repeat.off', player.cycleRepeat);

  const position = h('span', { class: 'player-time', text: '0:00' });
  const duration = h('span', { class: 'player-time', text: '0:00' });
  const progress = slider(0, 100, value => player.seek(value));

  const transport = h('div', { class: 'player-center' },
    h('div', { class: 'player-controls' }, shuffle, previous, play, next, repeat),
    h('div', { class: 'player-progress' }, position, progress, duration));

  const refresh = () => {
    if (!ui.nowPlaying) return;
    play.replaceChildren(icon(ui.playing ? 'pause' : 'play', 16, true));
    describe(play, ui.playing ? 'player.pause' : 'player.play');
    shuffle.classList.toggle('on', ui.shuffle);
    repeat.replaceChildren(icon(ui.repeat === 'one' ? 'repeat-one' : 'repeat'));
    repeat.classList.toggle('on', ui.repeat !== 'off');
    describe(repeat, `player.repeat.${ui.repeat}`);
    progress.max = String(ui.duration);
    if (!progress.matches(':active')) progress.value = String(ui.position);
    paintFilled(progress);
    position.textContent = player.formatTime(ui.position);
    duration.textContent = player.formatTime(ui.duration);
  };
  subscribeWhile(transport, 'player', refresh);
  refresh();
  return transport;
}

/** The volume icon and slider: the bottom bar's, and Reproduciendo's when the bar is not shown. */
export function buildVolume(): HTMLElement {
  const volume = slider(0, 1, value => player.setVolume(value), 0.01);
  // Saved once let go, not on every step of the drag.
  volume.addEventListener('change', player.saveVolume);
  const volumeIcon = h('span', { class: 'player-volume-icon', title: t('player.volume') });
  const box = h('div', { class: 'player-volume' }, volumeIcon, volume);
  const refresh = () => {
    volumeIcon.replaceChildren(icon(ui.volume === 0 ? 'muted' : 'volume'));
    volume.value = String(ui.volume);
    paintFilled(volume);
  };
  subscribeWhile(box, 'player', refresh);
  refresh();
  return box;
}

export function control(name: IconName, key: string, action: () => void, filled = false): HTMLButtonElement {
  return h('button', { class: 'player-control', title: t(key), onclick: action }, icon(name, 18, filled));
}

export function describe(button: HTMLElement, key: string): void {
  button.title = t(key);
  button.setAttribute('aria-label', t(key));
}

export function slider(min: number, max: number, changed: (value: number) => void, stepSize = 1): HTMLInputElement {
  const input = h('input', { class: 'player-slider', attrs: { type: 'range', min: String(min), max: String(max), step: String(stepSize) } });
  input.addEventListener('input', () => {
    paintFilled(input);
    changed(Number(input.value));
  });
  return input;
}

/** The part left of the knob in green: the time gone by, the volume set. */
export function paintFilled(input: HTMLInputElement): void {
  const min = Number(input.min);
  const range = Number(input.max) - min;
  const pct = range > 0 ? ((Number(input.value) - min) / range) * 100 : 0;
  input.style.setProperty('--filled', `${pct}%`);
}
