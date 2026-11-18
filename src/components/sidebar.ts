import { h } from '../dom';
import { appVersion } from '../appInfo';
import { t } from '../i18n';
import { icon, type IconName } from '../icons';
import { navigate, type Route } from '../router';
import { subscribeWhile, ui } from '../state';
import { openOverlay } from './overlay';
import { devicesButton } from './devicesButton';
import { themePicker } from './themePicker';
import { cover } from './cover';
import { homeLogo } from './logo';
import { findBlock } from '../library';
import * as player from '../player';
import { pipButton } from './pip';
import { control, describe, seekSlider } from './transport';

// `short`: the phone's tab, a fifth of the screen wide.
interface Entry { route: Route; label: string; icon: IconName; short?: string; }
const groups: { label?: string; entries: Entry[] }[] = [
  { entries: [{ route: 'home', label: 'nav.home', icon: 'home' }] },
  { label: 'nav.group.download', entries: [
    { route: 'download', label: 'nav.download', icon: 'download', short: 'nav.short.download' },
    { route: 'new-playlist', label: 'nav.newPlaylist', icon: 'new-playlist' },
  ] },
  { label: 'nav.group.library', entries: [
    { route: 'manager', label: 'nav.manager', icon: 'manager', short: 'nav.short.manager' },
    { route: 'player', label: 'nav.player', icon: 'player' },
  ] },
];
const buttons = new Map<Route, HTMLButtonElement>();

export function buildSidebar(): HTMLElement {
  const sidebar = h('nav', { class: 'panel sidebar' });
  const wordmark = h('span', { class: 'brand-name grow', text: 'Soundify' });
  const toggle = h('button', { class: 'sidebar-toggle', title: t('nav.toggle') }, icon('panel-left-close', 18));
  toggle.addEventListener('click', () => {
    ui.sidebarCollapsed = !ui.sidebarCollapsed;
    sidebar.classList.toggle('collapsed', ui.sidebarCollapsed);
  });
  const nav = h('div', { class: 'nav-list' });
  for (const group of groups) {
    if (group.label) nav.append(h('div', { class: 'group-label', text: t(group.label) }));
    for (const entry of group.entries) {
      const label = t(entry.label);
      // Crear Playlist shares the phone's Descargar tab (`formSwitch`).
      const button = h('button', { class: entry.route === 'new-playlist' ? 'nav-button phone-hide' : 'nav-button', title: label },
        entry.route === 'home' ? homeLogo(20) : icon(entry.icon, 20),
        h('span', { class: 'nav-label', text: label }),
        h('span', { class: 'nav-label-short', text: t(entry.short ?? entry.label) }));
      button.addEventListener('click', () => {
        if (entry.route !== 'player') return navigate(entry.route);
        if (ui.remote) return navigate('player');
        if (ui.lastPlayerPage === 'player' && ui.lastPlayerBlock) return navigate('player', { b: ui.lastPlayerBlock });
        navigate(ui.lastPlayerPage);
      });
      buttons.set(entry.route, button);
      nav.append(button);
    }
  }
  // With nothing playing the mini player is not there, and the devices button with it: this one takes its place, so «Dispositivos» is always within reach.
  const idleDevices = devicesButton('side-devices-idle');
  const footer = h('div', { class: 'sidebar-bottom' }, buildMiniPlayer(idleDevices),
    h('div', { class: 'sidebar-separator' }),
    navButton('settings', 'nav.settings', 'settings'),
    h('div', { class: 'sidebar-version' }, h('span', { class: 'sidebar-version-text grow', text: `${t('nav.version')} ${appVersion()}` }),
      idleDevices,
      h('button', { class: 'theme-button', title: t('settings.interface.theme'), onclick: () => openOverlay(t('settings.interface.theme'), [themePicker()]) }, icon('palette', 17))));
  sidebar.append(h('div', { class: 'sidebar-header' }, wordmark, toggle), nav, h('div', { class: 'grow' }), footer);
  sidebar.classList.toggle('collapsed', ui.sidebarCollapsed);
  return sidebar;
}
function navButton(route: Route, labelKey: string, iconName: IconName): HTMLButtonElement {
  const label = t(labelKey);
  const button = h('button', { class: 'nav-button', title: label }, icon(iconName, 20), h('span', { class: 'nav-label', text: label }), h('span', { class: 'nav-label-short', text: label }));
  button.addEventListener('click', () => navigate(route));
  buttons.set(route, button);
  return button;
}
export function markActive(route: Route): void {
  const active: Route = route === 'now-playing' || route === 'remote-playlist' || route === 'cast-queue' ? 'player' : route;
  buttons.forEach((button, r) => button.classList.toggle('selected', r === active));
  // The phone has one tab for both ways of downloading.
  buttons.get('download')?.classList.toggle('phone-selected', route === 'new-playlist');
}
function buildMiniPlayer(idleDevices: HTMLElement): HTMLElement {
  // Cover and title open Reproduciendo; the playlist's name, that playlist in the Reproductor.
  const art = h('div', { class: 'mini-cover', onclick: () => navigate('now-playing') });
  const title = h('b', { class: 'ellipsis player-link', onclick: () => navigate('now-playing') });
  const meta = h('span', { class: 'ellipsis player-link', title: t('nowPlaying.openPlaylist'), onclick: () => {
    if (ui.nowPlaying) navigate('player', { b: ui.nowPlaying.blockKey });
  } });
  const toggle = h('button', { class: 'mini-play', title: t('player.play'), onclick: player.togglePlay });
  const progress = seekSlider();
  progress.classList.add('mini-progress');
  // The basic controls, so the side menu alone is enough to drive what sounds (and to change the device it sounds on).
  const shuffle = control('shuffle', 'player.shuffle', player.toggleShuffle);
  const previous = control('previous', 'player.previous', player.previous, true);
  const next = control('next', 'player.next', player.next, true);
  const repeat = control('repeat', 'player.repeat.off', player.cycleRepeat);
  const controls = h('div', { class: 'mini-controls' }, devicesButton('side-devices'), shuffle, previous, next, repeat);
  const mini = h('div', { class: 'mini-player' }, art, h('div', { class: 'mini-text' }, title, meta), pipButton(), toggle, controls, progress);
  let shownKey = '';
  let shownRepeat = '';
  let shownPlaying: boolean | null = null;
  const refresh = () => {
    const np = ui.nowPlaying; mini.hidden = !np || !['side', 'both'].includes(ui.playerPlacement);
    idleDevices.classList.toggle('gone', !mini.hidden);
    if (!np) return;
    const found = findBlock(np.blockKey); title.textContent = np.track.title; meta.textContent = found?.playlist.title ?? '';
    // Only when the track changes: this runs on every tick, and a new tile would fetch its picture again.
    const key = `${np.blockKey}/${np.track.id}`;
    if (key !== shownKey) { shownKey = key; art.replaceChildren(cover(np.track.title, found?.block.video ?? false, 40, { kind: 'thumbnail', id: np.track.id })); }
    // Only when it changes, like repeat below: swapped on every tick, a press that spans one is lost.
    if (ui.playing !== shownPlaying) {
      shownPlaying = ui.playing;
      toggle.replaceChildren(icon(ui.playing ? 'pause' : 'play', 16, true));
      toggle.title = t(ui.playing ? 'player.pause' : 'player.play');
    }
    shuffle.classList.toggle('on', ui.shuffle);
    // Only when it changes: this runs with every tick, and a button whose content is swapped between press and release loses its click.
    if (ui.repeat !== shownRepeat) {
      shownRepeat = ui.repeat;
      repeat.replaceChildren(icon(ui.repeat === 'one' ? 'repeat-one' : 'repeat'));
      describe(repeat, `player.repeat.${ui.repeat}`);
    }
    repeat.classList.toggle('on', ui.repeat !== 'off');
    progress.style.setProperty('--mini-progress', `${ui.duration > 0 ? Math.min(100, ui.position / ui.duration * 100) : 0}%`);
  };
  subscribeWhile(mini, 'player', refresh); subscribeWhile(mini, 'player-placement', refresh); refresh(); return mini;
}


