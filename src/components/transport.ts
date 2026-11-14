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
  const progress = seekSlider();

  const transport = h('div', { class: 'player-center' },
    h('div', { class: 'player-controls' }, shuffle, previous, play, next, repeat),
    h('div', { class: 'player-progress' }, position, progress, duration));

  // The icons are only replaced when they change: this runs with every tick of the song, and a button whose content is
  // swapped between pressing and letting go of the mouse can lose its click.
  let shownPlay = '';
  let shownRepeat = '';
  const refresh = () => {
    if (!ui.nowPlaying) return;
    const playKey = ui.playing ? 'pause' : 'play';
    if (playKey !== shownPlay) {
      shownPlay = playKey;
      play.replaceChildren(icon(playKey, 16, true));
      describe(play, ui.playing ? 'player.pause' : 'player.play');
    }
    shuffle.classList.toggle('on', ui.shuffle);
    if (ui.repeat !== shownRepeat) {
      shownRepeat = ui.repeat;
      repeat.replaceChildren(icon(ui.repeat === 'one' ? 'repeat-one' : 'repeat'));
      describe(repeat, `player.repeat.${ui.repeat}`);
    }
    repeat.classList.toggle('on', ui.repeat !== 'off');
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

/**
 * The file's position, wherever it is shown: a click jumps there, dragging
 * moves it, the arrow keys step it. It is written only while not held, so the
 * clock's ticks do not pull it from under the pointer.
 */
export function seekSlider(): HTMLInputElement {
  const input = slider(0, 100, value => player.seek(value));
  input.setAttribute('aria-label', t('player.seek'));
  const refresh = () => {
    input.max = String(ui.duration);
    if (!input.matches(':active')) input.value = String(ui.position);
    paintFilled(input);
  };
  subscribeWhile(input, 'player', refresh);
  refresh();
  return input;
}

/** The volume icon and slider: the bottom bar's, and Reproduciendo's when the bar is not shown. */
export function buildVolume(): HTMLElement {
  const volume = slider(0, 1, value => player.setVolume(value), 0.01);
  // Saved once let go, not on every step of the drag.
  volume.addEventListener('change', player.saveVolume);
  const volumeIcon = h('span', { class: 'player-volume-icon', title: t('player.volume') });
  const box = h('div', { class: 'player-volume' }, volumeIcon, volume);
  let shownVolume = '';
  const refresh = () => {
    const volumeKey = ui.volume === 0 ? 'muted' : 'volume';
    if (volumeKey !== shownVolume) {
      shownVolume = volumeKey;
      volumeIcon.replaceChildren(icon(volumeKey));
    }
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
