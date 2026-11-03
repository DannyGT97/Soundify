import { h, titleRow } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { api, type FormatBlock, type Playlist } from '../api';
import { countBy, findBlock, library, loadLibrary, onDisk } from '../library';
import { navigate, onLeave, type Params } from '../router';
import { subscribe, ui, type RunningTask } from '../state';
import { liveText, percent, progressLine } from '../tasks';
import { cover } from '../components/cover';
import { playlistPicture } from '../covers';
import { confirmDialog } from '../overlays/confirm';
import { deviceLabel } from '../overlays/devicePicker';
import { openFilesOverlay } from '../overlays/files';
import { openImportHelp } from '../overlays/help';
import { copyToDevice, copyToFolder } from '../overlays/transfer';
import { notify } from '../components/toasts';
import { closeOverlay, openOverlay } from '../components/overlay';
import { errorText } from '../transfer';

/**
 * Gestor de Playlists: one folding row per playlist+folder, with a block per
 * format inside. The state shown is always the last one known: nothing asks
 * YouTube on opening.
 *
 * `open` unfolds a playlist on arrival; `focus` also scrolls to one of its
 * blocks («Ir a la descarga» of the tasks window).
 */
export function managerView(params: Params): HTMLElement {
  if (params.open) ui.expandedPlaylists.add(params.open);
  const list = h('div', { class: 'stack-14' });
  let shape = '';
  const render = () => {
    shape = liveShape();
    list.replaceChildren(...library.playlists.map(playlistRow));
  };
  // Every percent emits `tasks`: rebuilding the rows each time restarted the
  // outline's sweep and took the focus off a button under the pointer. Only a
  // change of shape (start, end, another block, another step) rebuilds; the
  // counter and the bars are written in place.
  onLeave(subscribe('tasks', () => liveShape() === shape ? patchLive(list) : render()));
  onLeave(subscribe('library', render));
  render();
  const focus = params.focus;
  if (focus) requestAnimationFrame(() => [...list.querySelectorAll<HTMLElement>('.pm-block')]
      .find(el => el.dataset.block === focus)?.scrollIntoView({ block: 'center', behavior: 'smooth' }));

  const importButton = h('button', { class: 'secondary-button', onclick: openImportHelp },
    icon('import', 16), h('span', { text: t('manager.import') }));

  const empty = h('section', { class: 'card stack-14' },
    h('p', { class: 'subtitle', text: t('manager.empty') }),
    h('p', { class: 'subtitle', text: t('manager.empty.recover') }),
    h('div', { class: 'row center-x' }, h('button', { class: 'secondary-button', text: t('manager.findPlaylists'), onclick: openImportHelp })));

  return h('div', { class: 'screen' },
    h('div', { class: 'screen-head' },
      titleRow('manager', t('manager.title'), h('div', { class: 'grow' }), importButton),
      h('p', { class: 'subtitle', text: t('manager.description') })),
    library.playlists.length ? list : empty);
}

// The row just unfolded by hand: only its blocks slide in. Rebuilt by a
// download's reloads, they slid in again every time and the Gestor flickered.
let unfolding: string | null = null;

function playlistRow(playlist: Playlist): HTMLElement {
  const expanded = ui.expandedPlaylists.has(playlist.id);
  const task = ui.task;
  const live = task?.blockKey ? playlist.formats.find(b => b.key === task.blockKey) ?? null : null;
  const files = playlist.formats.reduce((n, b) => n + onDisk(b.tracks).length, 0);
  const header = h('button', { class: 'pm-row-header', title: t(expanded ? 'manager.row.fold' : 'manager.row.unfold') },
    cover(playlist.title, playlist.formats.every(b => b.video), 56, playlistPicture(playlist)),
    h('div', { class: 'grow min0' },
      h('div', { class: 'row center gap-8 min0' },
        h('div', { class: 'card-title ellipsis', text: playlist.title }),
        task && live ? liveChip(task, live.format) : null),
      h('div', { class: 'subtitle ellipsis', text: playlist.folder }),
      h('div', { class: 'subtitle', text: `${t('manager.formats', playlist.formats.map(b => b.format).join(', '))} · ${t('manager.files', files)}` })),
    h('span', { class: `chevron${expanded ? ' open' : ''}` }, icon('chevron', 20)));
  const coverButton = header.querySelector<HTMLElement>('.cover');
  if (coverButton) {
    coverButton.classList.add('pm-cover-button');
    coverButton.setAttribute('role', 'button');
    coverButton.setAttribute('tabindex', '0');
    coverButton.setAttribute('aria-label', t('manager.menu.cover'));
    coverButton.title = t('manager.menu.cover');
    coverButton.addEventListener('click', event => {
      event.stopPropagation();
      void changeCover(playlist);
    });
    coverButton.addEventListener('keydown', event => {
      if (event.key === 'Enter') { event.preventDefault(); coverButton.click(); }
    });
  }
  header.addEventListener('click', () => {
    if (expanded) ui.expandedPlaylists.delete(playlist.id);
    else {
      ui.expandedPlaylists.add(playlist.id);
      unfolding = playlist.id;
    }
    navigate('manager');
  });
  const entering = unfolding === playlist.id;
  if (entering) unfolding = null;
  return h('section', { class: `card pm-row${live ? ' active' : ''}` }, header,
    expanded ? h('div', { class: `pm-blocks${entering ? ' entering' : ''}` }, ...playlist.formats.map(b => formatBlock(playlist, b, live)))
      : task && live ? liveBar(task, ' pm-row-bar') : null);
}

/** `live`: the block of this playlist being downloaded, if any; the others wait for it. */
function formatBlock(playlist: Playlist, block: FormatBlock, live: FormatBlock | null): HTMLElement {
  const task = ui.task;
  const active = task !== null && live === block;
  const pending = countBy(block.tracks, 'pending');
  const failed = countBy(block.tracks, 'failed');
  const unavailable = countBy(block.tracks, 'unavailable');
  const status = pending
      ? h('span', { class: 'warn-text', text: t('manager.status.pending', pending) })
      : h('span', { class: 'ok-text', text: t('manager.status.synced') });
  const extra = [
    failed ? t('manager.status.moreFailed', failed) : '',
    unavailable ? t('manager.status.moreUnavailable', unavailable) : '',
  ].filter(Boolean).join(' ');

  const update = active
      ? h('button', { class: 'secondary-button danger-text', onclick: () => api.stopDownload() },
        icon('stopped', 16, true), h('span', { text: t('tasks.stop') }))
      : h('button', { class: 'primary-button', onclick: () => api.startDownload({ kind: 'update', blockKey: block.key }) },
        icon('sync', 16), h('span', { text: t('manager.sync') }));
  if (!active) update.disabled = task !== null;
  const files = h('button', { class: 'secondary-button', onclick: () => openFilesOverlay(playlist, block) },
    icon('eye', 16), h('span', { text: t('manager.viewFiles') }));
  const numbered = h('span', {
    class: `pm-numbered-icon${block.numbered ? '' : ' off'}`,
    title: t(block.numbered ? 'manager.numbered' : 'manager.notNumbered'),
    attrs: { role: 'img', 'aria-label': t(block.numbered ? 'manager.numbered' : 'manager.notNumbered') },
  }, icon(block.numbered ? 'list-ordered' : 'list', 16));

  const element = h('div', { class: `pm-block${active ? ' active' : live ? ' waiting' : ''}` },
    h('div', { class: 'row center gap-10' },
      h('span', { class: 'chip', text: block.format }),
      h('span', { class: 'row-title', text: t('manager.files', onDisk(block.tracks).length) }),
      h('span', { class: 'subtitle', text: t('manager.lastSync', block.lastSync) }),
      h('div', { class: 'grow' }),
      active ? h('span', { class: 'pm-badge', text: t('manager.block.downloading') }) : null),
    h('div', { class: 'row center gap-8' }, status, extra ? h('span', { class: 'subtitle', text: extra }) : null,
      h('div', { class: 'grow' }),
      numbered),
    task && active ? h('div', { class: 'stack-6' },
      h('div', { class: 'row center gap-8' },
        h('span', { class: 'row-title', text: progressLine(task), attrs: { 'data-live': 'line' } }),
        h('div', { class: 'grow' }),
        h('span', { class: 'subtitle', text: percentText(task), attrs: { 'data-live': 'pct' } })),
      liveBar(task)) : null,
    h('div', { class: 'row center gap-8 pm-actions' }, update, files, copyMenu(playlist, block),
    h('button', { class: 'icon-button', title: t('manager.openFolder'), attrs: { 'aria-label': t('manager.openFolder') }, onclick: () => openFolder(playlist, block) }, icon('folder-open', 17)),
    h('button', { class: 'icon-button pm-delete', title: t('manager.menu.delete'), attrs: { 'aria-label': t('manager.menu.delete') }, onclick: () => deleteBlock(playlist, block) }, icon('delete', 17))));
  element.dataset.block = block.key;
  // Rebuilt, the animations would start over: the same clock for all keeps
  // the sweep and the dot where they were.
  if (active) element.style.setProperty('--live-phase', livePhase());
  const flash = finished?.key === block.key ? Date.now() - finished.at : Infinity;
  if (!active && flash < FLASH_MS) {
    element.classList.add('done');
    element.style.setProperty('--done-delay', `${FLASH_HOLD_MS - flash}ms`);
  }
  return element;
}

// === THE RUNNING DOWNLOAD ===

const SWEEP_MS = 2600;
// The green of a block that ended well stays a moment, then fades by itself.
const FLASH_HOLD_MS = 1200;
const FLASH_MS = FLASH_HOLD_MS + 3000;

let followed: string | null = null;
let finished: { key: string; at: number } | null = null;

/**
 * Follows the download even while the Gestor is not on screen: the playlist
 * being downloaded comes out unfolded the first time its block is known
 * (folding it by hand afterwards is respected), and a block that ends well is
 * remembered for its green flash.
 */
subscribe('tasks', () => {
  const key = ui.task?.blockKey ?? null;
  if (key && key !== followed) {
    const found = findBlock(key);
    if (found) ui.expandedPlaylists.add(found.playlist.id);
  }
  if (!ui.task && followed && ui.history[0]?.status === 'ok') finished = { key: followed, at: Date.now() };
  followed = key;
});

function liveShape(): string {
  const task = ui.task;
  return task ? `${task.blockKey ?? ''}|${task.step}` : '';
}

/** What changes with every percent, written over the rows already built. */
function patchLive(root: HTMLElement): void {
  const task = ui.task;
  if (!task) return;
  const pct = percent(task);
  root.querySelectorAll<HTMLElement>('[data-live]').forEach(el => {
    switch (el.dataset.live) {
      case 'chip': el.textContent = liveText(task, el.dataset.format ?? ''); break;
      case 'line': el.textContent = progressLine(task); break;
      case 'pct': el.textContent = percentText(task); break;
      case 'bar': if (pct !== null) el.style.width = `${pct}%`; break;
    }
  });
}

/** «Descargando MP3 · 3/12» next to the playlist's title, with a dot that breathes. */
function liveChip(task: RunningTask, format: string): HTMLElement {
  const text = h('span', { text: liveText(task, format), attrs: { 'data-live': 'chip', 'data-format': format } });
  const chip = h('span', { class: 'pm-live' }, h('span', { class: 'pm-live-dot' }), text);
  chip.style.setProperty('--live-phase', livePhase());
  return chip;
}

function liveBar(task: RunningTask, extraClass = ''): HTMLElement {
  const pct = percent(task);
  const fill = h('div', { class: 'progress-fill', attrs: { 'data-live': 'bar' } });
  if (pct !== null) fill.style.width = `${pct}%`;
  return h('div', { class: `progress${pct === null ? ' indeterminate' : ''}${extraClass}` }, fill);
}

function percentText(task: RunningTask): string {
  const pct = percent(task);
  return pct === null ? '' : `${pct}%`;
}

/** A negative delay that puts every live animation at the same point of the same clock. */
function livePhase(): string {
  return `-${Date.now() % SWEEP_MS}ms`;
}

/** One cover per playlist: every format of it shows the picked image. */
async function changeCover(playlist: Playlist): Promise<void> {
  const image = await api.pickCoverImage(t('cover.choose.title', playlist.title), t('cover.choose.filter'));
  const block = playlist.formats[0];
  if (!image || !block) return;
  try {
    await api.setCover(block.key, image);
    await loadLibrary();
  } catch {
    notify({ tone: 'error', text: t('cover.choose.failed') });
  }
}

/** A phone's folder opens in Windows' own explorer; a local one, in the system's. */
function openFolder(playlist: Playlist, block: FormatBlock): void {
  if (!block.device) {
    api.revealFolder(playlist.folder);
    return;
  }
  api.openDeviceFolder(block.device.deviceId, block.device.path)
    .catch(() => notify({ tone: 'error', text: t('manager.openFolder.deviceFailed') }));
}

/**
 * With no reachable folder, "Eliminar todo" is NOT offered: it would find not
 * one file to delete and would happily assume they are gone — with the USB
 * unplugged, that means losing the record of files that still exist.
 * Forgetting the record touches no file, so that option stays.
 */
function deleteBlock(playlist: Playlist, block: FormatBlock): void {
  const label = `${playlist.title} (${block.format})`;
  const accessible = block.accessible !== false;
  const location = block.device ? deviceLabel(block.device) : playlist.folder;
  const onlyRecord = { label: t('manager.delete.onlyRecord'), run: () => void forget(block) };
  confirmDialog(t('manager.delete.title'), t('manager.delete.header', label, block.tracks.length),
    accessible ? t('manager.delete.body', location) : t('manager.delete.bodyUnreachable'),
    accessible
      ? [{ label: t('manager.delete.all'), kind: 'danger', run: () => void deleteAll(playlist, block, location) }, onlyRecord]
      : [onlyRecord]);
}

async function forget(block: FormatBlock): Promise<void> {
  await api.deletePlaylistFormat(block.key, false);
  await loadLibrary();
}

// A local delete is fast enough to flash by unseen: the window stays at least
// this long, so it feels the same as a phone's (asked for).
const MIN_DELETE_WINDOW_MS = 600;

/**
 * Having asked for "delete everything", everything being deleted means it
 * went well: said in green and that is that. Only what could NOT be deleted
 * is warned about in red, the only thing left for the user to do.
 */
async function deleteAll(playlist: Playlist, block: FormatBlock, location: string): Promise<void> {
  const onDevice = block.device != null;
  openOverlay(t(onDevice ? 'manager.deleteDevice.windowTitle' : 'manager.deleteLocal.windowTitle'), [
    h('div', { class: 'stack-12' },
      h('div', { class: 'progress indeterminate' }, h('div', { class: 'progress-fill' })),
      h('div', { class: 'row-title ellipsis',
        text: t(onDevice ? 'manager.deleteDevice.inProgress' : 'manager.deleteLocal.inProgress', playlist.title) })),
  ]);
  const started = Date.now();
  try {
    const result = await api.deletePlaylistFormat(block.key, true);
    const wait = MIN_DELETE_WINDOW_MS - (Date.now() - started);
    if (wait > 0) await new Promise(resolve => window.setTimeout(resolve, wait));
    const total = (result?.deleted ?? 0) + (result?.unregistered ?? 0);
    notify(result && result.leftOver > 0
      ? { tone: 'error', text: t('manager.delete.leftOver', playlist.title, total, result.leftOver, location) }
      : { tone: 'success', text: t('manager.delete.done', playlist.title, block.format.toUpperCase(), total) });
  } catch (error) {
    notify({ tone: 'error', text: errorText(error) });
  } finally {
    closeOverlay();
    await loadLibrary();
  }
}

/** Copy destinations live under one compact action, with neutral icons. */
function copyMenu(playlist: Playlist, block: FormatBlock): HTMLElement {
  const menu = h('div', { class: 'popup-menu pm-copy-menu' },
    h('button', { onclick: () => {
      menu.hidden = true;
      void copyToFolder(playlist, block);
    } }, icon('folder-open', 16), h('span', { text: t(block.device ? 'manager.menu.copy.toComputer' : 'manager.menu.copy.local') })),
    block.device ? null : h('button', { onclick: () => {
      menu.hidden = true;
      copyToDevice(playlist, block);
    } }, icon('device', 16), h('span', { text: t('manager.menu.copy.device') })));
  menu.hidden = true;
  const button = h('button', { class: 'secondary-button pm-copy-button', onclick: e => {
    e.stopPropagation();
    const open = menu.hidden;
    document.querySelectorAll<HTMLElement>('.pm-copy-menu').forEach(m => { m.hidden = true; });
    menu.hidden = !open;
  } }, icon('copy', 16), h('span', { text: t('manager.copy.button') }));
  return h('div', { class: 'menu-anchor' }, button, menu);
}

document.addEventListener('click', () => {
  document.querySelectorAll<HTMLElement>('.pm-copy-menu').forEach(m => { m.hidden = true; });
});
