import { h, titleRow } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { api, type FormatBlock, type Playlist } from '../api';
import { countBy, filesText, findBlock, library, loadLibrary, onDisk, pendingText } from '../library';
import { navigate, onLeave, type Params } from '../router';
import { subscribe, ui } from '../state';
import { filterByTitle } from '../search';
import { progressLine, queuedFor, runningFor } from '../tasks';
import { cover } from '../components/cover';
import { openMenu, type MenuItem } from '../components/floating';
import { liveBar, liveChip, livePhase, patchLive, percentText } from '../components/live';
import { playlistPicture, refreshPickedCovers } from '../covers';
import { openDeleteWindow } from '../overlays/deletePlaylist';
import { deviceLabel } from '../overlays/devicePicker';
import { openFilesOverlay } from '../overlays/files';
import { openImportHelp } from '../overlays/help';
import { copyToDevice, copyToFolder } from '../overlays/transfer';
import { notify } from '../components/toasts';
import { errorText } from '../transfer';
import { canDownload, tools } from '../tools';
import { toolsNotice } from '../components/toolsNotice';

type Filter = 'all' | 'pending' | 'device';

const missing = (block: FormatBlock) => countBy(block.tracks, 'pending') + countBy(block.tracks, 'failed');

const IN_FILTER: Record<Filter, (playlist: Playlist) => boolean> = {
  all: () => true,
  pending: playlist => playlist.formats.some(block => missing(block) > 0),
  device: playlist => playlist.formats.some(block => block.device != null),
};

/**
 * Gestor de Playlists: one folding row per playlist+folder, with a block per
 * format inside. The state shown is always the last one known: nothing asks
 * YouTube on opening.
 *
 * `open` unfolds a playlist on arrival; `focus` also scrolls to one of its
 * blocks («Ir a la descarga»); `filter=pending` arrives with that chip on.
 */
export function managerView(params: Params): HTMLElement {
  if (params.open) ui.expandedPlaylists.add(params.open);
  let query = '';
  let filter: Filter = params.filter === 'pending' ? 'pending' : 'all';
  const list = h('div', { class: 'pm-list' });

  const search = h('input', { class: 'input', attrs: { type: 'search', placeholder: t('manager.search') } });
  search.addEventListener('input', () => {
    query = search.value;
    render();
  });
  const setFilter = (next: Filter) => {
    filter = next;
    render();
  };
  const chips: Record<Filter, HTMLButtonElement> = {
    all: h('button', { class: 'fchip', text: t('manager.filter.all'), onclick: () => setFilter('all') }),
    pending: h('button', { class: 'fchip', text: t('manager.filter.pending'), onclick: () => setFilter('pending') }),
    device: h('button', { class: 'fchip', text: t('manager.filter.device'), onclick: () => setFilter('device') }),
  };
  const toolbar = h('div', { class: 'toolbar' },
    h('div', { class: 'search-box' }, icon('search', 15), search),
    chips.all, chips.pending, chips.device);

  let shape = '';
  // The list only: the search field keeps its focus while typing. Empty or not
  // is decided on every render, because importing from the empty screen fills it.
  function render(): void {
    shape = liveShape();
    const all = library.playlists;
    toolbar.hidden = all.length === 0;
    chips.device.hidden = !all.some(IN_FILTER.device);
    if (filter === 'device' && chips.device.hidden) filter = 'all';
    (Object.keys(chips) as Filter[]).forEach(key => chips[key].classList.toggle('on', key === filter));
    if (!all.length) {
      list.replaceChildren(emptyManager());
      return;
    }
    const shown = filterByTitle(all, query).filter(IN_FILTER[filter]);
    list.replaceChildren(...(shown.length
      ? shown.map(playlist => playlistRow(playlist, render))
      : [h('p', { class: 'none', text: t('manager.search.none') })]));
  }
  // Every percent emits `tasks`: rebuilding the rows each time restarted the
  // outline's sweep and took the focus off a button under the pointer. Only a
  // change of shape (start, end, another block, another step) rebuilds; the
  // counter and the bars are written in place.
  onLeave(subscribe('tasks', () => liveShape() === shape ? patchLive(list) : render()));
  onLeave(subscribe('library', render));
  // `tools` also fires with every percent of an install: the same rule.
  onLeave(subscribe('tools', () => { if (liveShape() !== shape) render(); }));
  render();
  const focus = params.focus;
  if (focus) requestAnimationFrame(() => [...list.querySelectorAll<HTMLElement>('.pm-block')]
    .find(el => el.dataset.block === focus)?.scrollIntoView({ block: 'center', behavior: 'smooth' }));

  const importButton = h('button', { class: 'secondary-button', onclick: openImportHelp },
    icon('import', 16), h('span', { text: t('manager.import') }));

  return h('div', { class: 'screen' },
    h('div', { class: 'screen-head' },
      titleRow('manager', t('manager.title'), h('div', { class: 'grow' }), importButton)),
    toolsNotice(),
    toolbar,
    list);
}

function emptyManager(): HTMLElement {
  return h('div', { class: 'empty' },
    h('div', { class: 'empty-mark' }, icon('manager', 32)),
    h('h2', { text: t('manager.empty.title') }),
    h('p', { text: t('manager.empty.body') }),
    h('div', { class: 'row gap-8' },
      h('button', { class: 'primary-button', onclick: () => navigate('new-playlist') }, icon('new-playlist', 16), h('span', { text: t('nav.newPlaylist') })),
      h('button', { class: 'secondary-button', onclick: openImportHelp }, icon('import', 16), h('span', { text: t('manager.import') }))));
}

// The row just unfolded by hand: only its blocks slide in. Rebuilt by a
// download's reloads, they slid in again every time and the Gestor flickered.
let unfolding: string | null = null;

function playlistRow(playlist: Playlist, rerender: () => void): HTMLElement {
  const expanded = ui.expandedPlaylists.has(playlist.id);
  // The block of this playlist that is downloading (with two at once, each playlist has its own).
  const live = playlist.formats.find(b => runningFor(b.key)) ?? null;
  const task = live ? runningFor(live.key) : null;
  const files = playlist.formats.reduce((n, b) => n + onDisk(b.tracks).length, 0);
  const waiting = playlist.formats.reduce((n, b) => n + missing(b), 0);
  const deviceBlock = playlist.formats.find(b => b.device);
  const offline = playlist.formats.every(b => b.accessible === false);
  const where = deviceBlock?.device ? deviceLabel(deviceBlock.device) : playlist.folder;

  const coverButton = h('span', {
    class: 'pm-cover',
    title: t('manager.menu.cover'),
    attrs: { role: 'button', tabindex: '0' },
  },
    cover(playlist.title, playlist.formats.every(b => b.video), 52, playlistPicture(playlist)),
    h('span', { class: 'shade' }, icon('image', 18)));
  coverButton.addEventListener('click', event => {
    event.stopPropagation();
    void changeCover(playlist);
  });
  coverButton.addEventListener('keydown', event => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    event.stopPropagation();
    void changeCover(playlist);
  });

  const toggle = () => {
    if (expanded) ui.expandedPlaylists.delete(playlist.id);
    else {
      ui.expandedPlaylists.add(playlist.id);
      unfolding = playlist.id;
    }
    rerender();
  };
  const header = h('div', {
    class: 'pm-row-header',
    title: t(expanded ? 'manager.row.fold' : 'manager.row.unfold'),
    attrs: { role: 'button', tabindex: '0', 'aria-expanded': String(expanded) },
    onclick: toggle,
  },
    coverButton,
    h('div', { class: 'grow min0' },
      h('div', { class: 'row center gap-8 min0' },
        h('div', { class: 'pm-title ellipsis', text: playlist.title }),
        task && live ? liveChip(task, live.format) : null),
      h('div', { class: 'pm-sub', title: where },
        icon(deviceBlock ? 'device' : 'folder', 13),
        h('span', { class: 'ellipsis', text: where }))),
    h('div', { class: 'pm-chips' }, ...playlist.formats.map(b => h('span', { class: 'fmt', text: b.format.toUpperCase() }))),
    h('div', { class: 'pm-state' },
      h('small', { text: filesText(files) }),
      offline ? h('span', { class: 'warn-text', text: t('manager.status.offline') })
        : waiting ? h('span', { class: 'warn-text', text: pendingText(waiting) })
        : h('span', { class: 'ok-text', title: t('manager.status.upToDate') }, icon('ok', 15))),
    h('span', { class: `chevron${expanded ? ' open' : ''}` }, icon('chevron', 20)));
  header.addEventListener('keydown', event => {
    if (event.key === 'Enter' && event.target === header) {
      event.preventDefault();
      toggle();
    }
  });

  const entering = unfolding === playlist.id;
  if (entering) unfolding = null;
  return h('section', { class: `pm-row${live ? ' active' : ''}` }, header,
    expanded ? h('div', { class: `pm-blocks${entering ? ' entering' : ''}` }, ...playlist.formats.map(b => formatBlock(playlist, b)))
      : task && live ? liveBar(task, ' pm-row-bar') : null);
}

function formatBlock(playlist: Playlist, block: FormatBlock): HTMLElement {
  const task = runningFor(block.key);
  const active = task !== null;
  const pending = countBy(block.tracks, 'pending');
  const failed = countBy(block.tracks, 'failed');
  const unavailable = countBy(block.tracks, 'unavailable');
  const offline = block.accessible === false;

  const sub: Node[] = [];
  const add = (node: HTMLElement) => {
    if (sub.length) sub.push(document.createTextNode(' · '));
    sub.push(node);
  };
  if (offline) add(h('span', { class: 'warn-text', text: t('manager.status.offline') }));
  else if (pending) add(h('span', { class: 'warn-text', text: pendingText(pending) }));
  else if (!failed) add(h('span', { class: 'ok-text', text: t('manager.status.upToDate') }));
  if (failed) add(h('span', { class: 'err-text', text: t('manager.status.failed', failed) }));
  if (unavailable) add(h('span', { text: t('manager.status.unavailable', unavailable) }));
  if (block.lastSync) add(h('span', { text: t('manager.lastSync', block.lastSync) }));

  const worthSyncing = pending + failed > 0;
  const queued = queuedFor(block.key);
  const update = active
    ? h('button', { class: 'secondary-button danger-text', onclick: () => api.stopDownload(task?.id) },
      icon('stopped', 16, true), h('span', { text: t('tasks.stop') }))
    : queued
    ? h('button', { class: 'secondary-button', title: t('tasks.queue.remove'), onclick: () => api.removeQueued(queued.id) },
      icon('close', 16), h('span', { text: t('tasks.queue.remove') }))
    : h('button', {
      class: worthSyncing ? 'primary-button' : 'secondary-button compactable',
      title: t('manager.sync'),
      onclick: () => { if (canDownload(tools.report)) api.startDownload({ kind: 'update', blockKey: block.key }); },
    }, icon('sync', 16), h('span', { text: t('manager.sync') }));
  if (!active && !queued) update.disabled = !canDownload(tools.report) || offline;

  const element = h('div', { class: `pm-block${active ? ' active' : ''}` },
    h('div', { class: 'fl-main' },
      h('span', { class: 'fmt', text: block.format.toUpperCase() }),
      h('div', { class: 'grow min0' },
        h('div', { class: 'fl-title' },
          h('span', { text: filesText(onDisk(block.tracks).length) }),
          h('span', {
            class: 'row center',
            title: t(block.numbered ? 'manager.numbered' : 'manager.notNumbered'),
            attrs: { role: 'img' },
          }, icon(block.numbered ? 'list-ordered' : 'list', 14)),
          active ? h('span', { class: 'pm-badge', text: t('manager.block.downloading') }) : null,
          queued ? h('span', { class: 'pm-badge queued', text: t('tasks.queue.badge') }) : null),
        h('div', { class: 'fl-sub ellipsis' }, ...sub)),
      h('div', { class: 'fl-actions' },
        update,
        h('button', { class: 'secondary-button compactable', title: t('manager.viewFiles'), onclick: () => openFilesOverlay(playlist, block) },
          icon('eye', 16), h('span', { text: t('manager.viewFiles') })),
        moreButton(playlist, block))),
    task && active ? h('div', { class: 'fl-live' },
      h('div', { class: 'row center gap-8' },
        h('span', { text: progressLine(task), attrs: { 'data-live': 'line', 'data-task': String(task.id) } }),
        h('div', { class: 'grow' }),
        h('span', { text: percentText(task), attrs: { 'data-live': 'pct', 'data-task': String(task.id) } })),
      liveBar(task)) : null);
  element.dataset.block = block.key;
  // Rebuilt, the animations would start over: the same clock for all keeps
  // the sweep and the dot where they were.
  if (active) element.style.setProperty('--live-phase', livePhase());
  const flash = Date.now() - (finished.get(block.key) ?? -Infinity);
  if (!active && flash < FLASH_MS) {
    element.classList.add('done');
    element.style.setProperty('--done-delay', `${FLASH_HOLD_MS - flash}ms`);
  }
  return element;
}

/** «⋯»: everything a block can do that is not an everyday thing. */
function moreButton(playlist: Playlist, block: FormatBlock): HTMLElement {
  const button = h('button', { class: 'icon-button pm-more', title: t('manager.menu.more') }, icon('more', 18));
  button.addEventListener('click', () => {
    const copies: MenuItem[] = [
      { icon: 'copy', label: t(block.device ? 'manager.menu.copy.toComputer' : 'manager.menu.copy.local'), run: () => void copyToFolder(playlist, block) },
    ];
    if (!block.device) copies.push({ icon: 'device', label: t('manager.menu.copy.device'), run: () => copyToDevice(playlist, block) });
    openMenu(button, [
      ...copies,
      { icon: 'folder-open', label: t('manager.openFolder'), run: () => openFolder(playlist, block) },
      null,
      { icon: 'delete', label: t('manager.menu.delete'), danger: true, run: () => openDeleteWindow(playlist, block) },
    ]);
  });
  return button;
}

// === THE RUNNING DOWNLOAD ===

// The green of a block that ended well stays a moment, then fades by itself.
const FLASH_HOLD_MS = 1200;
const FLASH_MS = FLASH_HOLD_MS + 3000;

let followed = new Set<string>();
/** Block → when it ended well (for the green flash). */
const finished = new Map<string, number>();

/**
 * Follows the download even while the Gestor is not on screen: the playlist
 * being downloaded comes out unfolded the first time its block is known
 * (folding it by hand afterwards is respected), and a block that ends well is
 * remembered for its green flash.
 */
subscribe('tasks', () => {
  const keys = new Set(ui.running.flatMap(task => (task.blockKey ? [task.blockKey] : [])));
  for (const key of keys) {
    if (followed.has(key)) continue;
    const found = findBlock(key);
    if (found) ui.expandedPlaylists.add(found.playlist.id);
  }
  // The ones that stopped running: green if the history says that playlist ended well.
  for (const key of followed) {
    if (keys.has(key)) continue;
    const title = findBlock(key)?.playlist.title;
    const record = ui.history.find(item => item.title === title) ?? ui.history[0];
    if (record?.status === 'ok') finished.set(key, Date.now());
  }
  followed = keys;
});

function liveShape(): string {
  const running = ui.running.map(task => `${task.id}:${task.blockKey ?? ''}:${task.step}`).join(',');
  return `${canDownload(tools.report)}|${running}|${ui.queue.map(q => q.id).join(',')}`;
}

/** One cover per playlist: every format of it shows the picked image. */
export async function changeCover(playlist: Playlist): Promise<void> {
  const block = playlist.formats[0];
  try {
    const image = await api.pickCoverImage(t('cover.choose.title', playlist.title), t('cover.choose.filter'));
    if (!image || !block) return;
    await api.setCover(block.key, image);
    // Every picked cover is called `cover.jpg`: the webview would keep showing the old one.
    refreshPickedCovers();
    await loadLibrary();
  } catch (error) {
    console.warn('cover: could not change it', error);
    notify({ tone: 'error', text: t('cover.choose.failed') });
  }
}

/** A phone's folder opens in Windows' own explorer; a local one, in the system's. */
export function openFolder(playlist: Playlist, block: FormatBlock): void {
  if (!block.device) {
    api.revealFolder(playlist.folder)
      .catch(error => notify({ tone: 'error', text: errorText(error) }));
    return;
  }
  api.openDeviceFolder(block.device.deviceId, block.device.path)
    .catch(() => notify({ tone: 'error', text: t('manager.openFolder.deviceFailed') }));
}
