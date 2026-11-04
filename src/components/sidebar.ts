import { h } from '../dom';
import { appVersion } from '../appInfo';
import { t } from '../i18n';
import { icon, type IconName } from '../icons';
import { navigate, type Route } from '../router';
import { subscribeWhile, ui } from '../state';
import { percent, statusMark } from '../tasks';
import { openTasksOverlay } from '../overlays/tasks';
import { openOverlay } from './overlay';
import { themePicker } from './themePicker';
import { cover } from './cover';
import { homeLogo } from './logo';
import { findBlock } from '../library';
import * as player from '../player';
import { pipButton } from './pip';
import { seekSlider } from './transport';

interface Entry { route: Route; label: string; icon: IconName; }
const groups: { label?: string; entries: Entry[] }[] = [
  { entries: [{ route: 'home', label: 'nav.home', icon: 'home' }] },
  { label: 'nav.group.download', entries: [
    { route: 'download', label: 'nav.download', icon: 'download' },
    { route: 'new-playlist', label: 'nav.newPlaylist', icon: 'new-playlist' },
  ] },
  { label: 'nav.group.library', entries: [
    { route: 'manager', label: 'nav.manager', icon: 'manager' },
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
      const button = h('button', { class: 'nav-button', title: label },
        entry.route === 'home' ? homeLogo(20) : icon(entry.icon, 20),
        h('span', { class: 'nav-label', text: label }));
      button.addEventListener('click', () => navigate(entry.route === 'player' ? ui.lastPlayerPage : entry.route));
      buttons.set(entry.route, button);
      nav.append(button);
    }
  }
  const footer = h('div', { class: 'sidebar-bottom' }, buildTasksPill(), buildMiniPlayer(),
    h('div', { class: 'sidebar-separator' }),
    navButton('settings', 'nav.settings', 'settings'),
    h('div', { class: 'sidebar-version' }, h('span', { class: 'sidebar-version-text grow', text: `${t('nav.version')} ${appVersion()}` }),
      h('button', { class: 'theme-button', title: t('settings.interface.theme'), onclick: () => openOverlay(t('settings.interface.theme'), [themePicker()]) }, icon('palette', 17))));
  sidebar.append(h('div', { class: 'sidebar-header' }, wordmark, toggle), nav, h('div', { class: 'grow' }), footer);
  sidebar.classList.toggle('collapsed', ui.sidebarCollapsed);
  return sidebar;
}
function navButton(route: Route, labelKey: string, iconName: IconName): HTMLButtonElement {
  const label = t(labelKey);
  const button = h('button', { class: 'nav-button', title: label }, icon(iconName, 20), h('span', { class: 'nav-label', text: label }));
  button.addEventListener('click', () => navigate(route));
  buttons.set(route, button);
  return button;
}
export function markActive(route: Route): void {
  const active: Route = route === 'now-playing' ? 'player' : route;
  buttons.forEach((button, r) => button.classList.toggle('selected', r === active));
}
function buildTasksPill(): HTMLElement {
  const ring = h('div', { class: 'tasks-ring' });
  const title = h('div', { class: 'tasks-pill-title' });
  const sub = h('div', { class: 'tasks-pill-sub' });
  const pill = h('button', { class: 'tasks-pill', title: t('tasks.title'), onclick: openTasksOverlay }, ring, h('div', { class: 'tasks-pill-text' }, title, sub));
  const refresh = () => {
    const task = ui.task; pill.hidden = !task && !ui.unseenResult;
    if (task) { const pct = percent(); ring.replaceChildren(progressRing(pct)); title.textContent = task.step === 'retry' ? t('tasks.pill.retry') : pct === null ? t('tasks.pill.preparing') : t('tasks.pill.downloading', pct); sub.textContent = task.title; }
    else if (ui.unseenResult) { const status = ui.unseenResult, mark = statusMark(status); ring.replaceChildren(h('span', { class: `mark-${status}` }, icon(mark.icon, 18))); title.textContent = t(`tasks.pill.${status}`); sub.textContent = ui.history[0]?.title ?? ''; }
  };
  subscribeWhile(pill, 'tasks', refresh); refresh(); return pill;
}
function progressRing(pct: number | null): SVGSVGElement {
  const ns = 'http://www.w3.org/2000/svg', svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 26 26'); svg.setAttribute('width', '26'); svg.setAttribute('height', '26');
  const radius = 10, length = 2 * Math.PI * radius;
  for (const cls of ['ring-track', 'ring-arc']) { const circle = document.createElementNS(ns, 'circle'); circle.setAttribute('cx', '13'); circle.setAttribute('cy', '13'); circle.setAttribute('r', String(radius)); circle.setAttribute('class', cls); if (cls === 'ring-arc') { const shown = pct === null ? .25 : pct / 100; circle.setAttribute('stroke-dasharray', `${length * shown} ${length}`); } svg.append(circle); }
  if (pct === null) svg.classList.add('ring-spin'); return svg;
}
function buildMiniPlayer(): HTMLElement {
  // Cover and title open Reproduciendo; the playlist's name, that playlist in the Reproductor.
  const art = h('div', { class: 'mini-cover', onclick: () => navigate('now-playing') });
  const title = h('b', { class: 'ellipsis player-link', onclick: () => navigate('now-playing') });
  const meta = h('span', { class: 'ellipsis player-link', title: t('nowPlaying.openPlaylist'), onclick: () => {
    if (ui.nowPlaying) navigate('player', { b: ui.nowPlaying.blockKey });
  } });
  const toggle = h('button', { class: 'mini-play', title: t('player.play'), onclick: player.togglePlay });
  const progress = seekSlider();
  progress.classList.add('mini-progress');
  const mini = h('div', { class: 'mini-player' }, art, h('div', { class: 'mini-text' }, title, meta), pipButton(), toggle, progress);
  let shownKey = '';
  const refresh = () => {
    const np = ui.nowPlaying; mini.hidden = !np || !['side', 'both'].includes(ui.playerPlacement); if (!np) return;
    const found = findBlock(np.blockKey); title.textContent = np.track.title; meta.textContent = found?.playlist.title ?? '';
    // Only when the track changes: this runs on every tick, and a new tile would fetch its picture again.
    const key = `${np.blockKey}/${np.track.id}`;
    if (key !== shownKey) { shownKey = key; art.replaceChildren(cover(np.track.title, found?.block.video ?? false, 40, { kind: 'thumbnail', id: np.track.id })); }
    toggle.replaceChildren(icon(ui.playing ? 'pause' : 'play', 16, true));
    toggle.title = t(ui.playing ? 'player.pause' : 'player.play');
    progress.style.setProperty('--mini-progress', `${ui.duration > 0 ? Math.min(100, ui.position / ui.duration * 100) : 0}%`);
  };
  subscribeWhile(mini, 'player', refresh); subscribeWhile(mini, 'player-placement', refresh); refresh(); return mini;
}


