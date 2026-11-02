// The player bar at the bottom of every screen. Visible only while something
// is loaded; its ✕ stops and unloads, which hides it again.

import { h } from '../dom';
import { t } from '../i18n';
import { icon, type IconName } from '../icons';
import { findBlock } from '../library';
import * as player from '../player';
import { navigate } from '../router';
import { subscribe, ui } from '../state';
import { cover } from './cover';

export function buildPlayerBar(): HTMLElement {
  const coverBox = h('div', { class: 'player-cover' });
  const title = h('div', { class: 'row-title ellipsis' });
  const meta = h('div', { class: 'subtitle ellipsis' });
  const now = h('div', { class: 'player-now', onclick: () => navigate('now-playing') }, coverBox,
    h('div', { class: 'player-now-text' }, title, meta));

  const shuffle = control('shuffle', 'player.shuffle', player.toggleShuffle);
  const previous = control('previous', 'player.previous', player.previous, true);
  const play = h('button', { class: 'player-play', onclick: player.togglePlay });
  const next = control('next', 'player.next', player.next, true);
  const repeat = control('repeat', 'player.repeat.off', player.cycleRepeat);

  const position = h('span', { class: 'player-time', text: '0:00' });
  const duration = h('span', { class: 'player-time', text: '0:00' });
  const progress = slider(0, 100, value => player.seek(value));
  const volume = slider(0, 1, value => player.setVolume(value), 0.01);
  const volumeIcon = h('span', { class: 'player-volume-icon', title: t('player.volume') });

  const bar = h('footer', { class: 'player-bar' },
    now,
    h('div', { class: 'player-center' },
      h('div', { class: 'player-controls' }, shuffle, previous, play, next, repeat),
      h('div', { class: 'player-progress' }, position, progress, duration)),
    h('div', { class: 'player-right' },
      volumeIcon, volume,
      control('close', 'player.close', player.close)));

  let shownKey = '';
  const refresh = () => {
    const np = ui.nowPlaying;
    bar.hidden = !np || !['bottom', 'both'].includes(ui.playerPlacement);
    if (!np) return;
    const found = findBlock(np.blockKey);
    title.textContent = np.track.title;
    meta.textContent = found ? `${found.playlist.title} · ${found.block.format}` : '';
    const key = `${np.blockKey}/${np.track.id}`;
    if (key !== shownKey) {
      shownKey = key;
      coverBox.replaceChildren(cover(np.track.title, found?.block.video ?? false, 48));
    }
    play.replaceChildren(icon(ui.playing ? 'pause' : 'play', 16, true));
    describe(play, ui.playing ? 'player.pause' : 'player.play');
    shuffle.classList.toggle('on', ui.shuffle);
    repeat.replaceChildren(icon(ui.repeat === 'one' ? 'repeat-one' : 'repeat'));
    volumeIcon.replaceChildren(icon(ui.volume === 0 ? 'muted' : 'volume'));
    repeat.classList.toggle('on', ui.repeat !== 'off');
    describe(repeat, `player.repeat.${ui.repeat}`);
    progress.max = String(np.track.seconds);
    if (!progress.matches(':active')) progress.value = String(ui.position);
    paintFilled(progress);
    volume.value = String(ui.volume);
    paintFilled(volume);
    position.textContent = player.formatTime(ui.position);
    duration.textContent = player.formatTime(np.track.seconds);
  };
  subscribe('player', refresh);
  subscribe('player-placement', refresh);
  refresh();
  return bar;
}

function control(name: IconName, key: string, action: () => void, filled = false): HTMLButtonElement {
  return h('button', { class: 'player-control', title: t(key), onclick: action }, icon(name, 18, filled));
}

function describe(button: HTMLElement, key: string): void {
  button.title = t(key);
  button.setAttribute('aria-label', t(key));
}

function slider(min: number, max: number, changed: (value: number) => void, stepSize = 1): HTMLInputElement {
  const input = h('input', { class: 'player-slider', attrs: { type: 'range', min: String(min), max: String(max), step: String(stepSize) } });
  input.addEventListener('input', () => {
    paintFilled(input);
    changed(Number(input.value));
  });
  return input;
}

/** The part left of the knob in green: the time gone by, the volume set. */
function paintFilled(input: HTMLInputElement): void {
  const min = Number(input.min);
  const range = Number(input.max) - min;
  const pct = range > 0 ? ((Number(input.value) - min) / range) * 100 : 0;
  input.style.setProperty('--filled', `${pct}%`);
}
