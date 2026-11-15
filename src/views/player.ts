import { h, titleRow } from '../dom';
import { icon, type IconName } from '../icons';
import { t } from '../i18n';
import { api, type FormatBlock, type Playlist, type Track } from '../api';
import { countBy, filesText, findBlock, groupByStatus, library, onDisk, statusText } from '../library';
import * as player from '../player';
import { formatTime, isPlayable } from '../player';
import { filterByTitle } from '../search';
import { nextOrder, orderOf, setOrder, sortTracks, type SortKey } from '../trackOrder';
import { ago } from '../relativeTime';
import { navigate, onLeave, type Params } from '../router';
import { subscribe, ui } from '../state';
import { cover } from '../components/cover';
import { connect } from '../connect';
import { openMenu } from '../components/floating';
import { liveBar, liveChip, patchLive } from '../components/live';
import { queuedFor, runningFor } from '../tasks';
import { canDownload, tools } from '../tools';
import { canPlayTrack, patchPlaying, trackRow } from '../components/trackRow';
import { hueOf, playlistPicture, queuePicture } from '../covers';
import { changeCover, openFolder } from './manager';

/**
 * Reproductor: the playlists as cards, or one of them (`?b=<block>`) with its
 * files. Pressing a file plays it and stays on the list; Reproduciendo opens
 * from the bar. The list of one playlist is also where its files are seen by state
 * (`?f=pending|off|ok` opens it filtered) and synced, so the Gestor sends «Abrir en
 * el Reproductor» here (`?from=manager` makes the arrow go back to it).
 */
export function playerView(params: Params): HTMLElement {
  ui.lastPlayerPage = 'player';
  return params.b && findBlock(params.b) ? detailView(params.b, params) : libraryView();
}

function relative(days: number): string {
  const { key, amount } = ago(days);
  return t(key, amount);
}

const canPlayBlock = (block: FormatBlock) => block.tracks.some(track => canPlayTrack(block, track));
const firstPlayable = (playlist: Playlist) => playlist.formats.find(canPlayBlock) ?? null;
const newest = (playlist: Playlist) => Math.min(Infinity, ...playlist.formats.flatMap(block => onDisk(block.tracks).map(track => track.addedDaysAgo)));
const size = (playlist: Playlist) => playlist.formats.reduce((n, block) => n + onDisk(block.tracks).length, 0);
const isOffline = (playlist: Playlist) => playlist.formats.every(block => block.accessible === false);

// === THE LIBRARY ===

type Chip = 'all' | 'audio' | 'video' | 'device';
type Sort = 'recent' | 'az' | 'size';

const IN_CHIP: Record<Chip, (playlist: Playlist) => boolean> = {
  all: () => true,
  audio: playlist => playlist.formats.some(block => !block.video),
  video: playlist => playlist.formats.some(block => block.video),
  device: playlist => playlist.formats.some(block => block.device != null),
};

const SORTS: Record<Sort, (a: Playlist, b: Playlist) => number> = {
  recent: (a, b) => newest(a) - newest(b),
  az: (a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: 'base' }),
  size: (a, b) => size(b) - size(a),
};

// Going into a playlist and back finds the list as it was left.
let query = '';
let chip: Chip = 'all';
let sort: Sort = 'recent';

function libraryView(): HTMLElement {
  const search = h('input', { class: 'input', attrs: { type: 'search', placeholder: t('player.library.search') } });
  search.value = query;
  const chipButtons = (['all', 'audio', 'video', 'device'] as Chip[]).map(key => {
    const button = h('button', { class: 'fchip', text: t(`player.library.chip.${key}`), onclick: () => {
      chip = key;
      fill();
    } });
    button.dataset.chip = key;
    return button;
  });
  const sortSelect = h('select', { class: 'sort', title: t('player.library.sort') },
    ...(['recent', 'az', 'size'] as Sort[]).map(key => h('option', { text: t(`player.library.sort.${key}`), attrs: { value: key } })));
  sortSelect.value = sort;
  sortSelect.addEventListener('change', () => {
    sort = sortSelect.value as Sort;
    fill();
  });
  search.addEventListener('input', () => {
    query = search.value;
    fill();
  });

  const resumeSlot = h('div');
  // While another device is controlled, its playlist is here too (with the device it is on), above this device's own.
  const remoteSlot = h('div');
  const fillRemote = () => {
    const device = ui.remote;
    if (!device) {
      remoteSlot.replaceChildren();
      return;
    }
    const queue = connect.queue;
    const card = h('div', { class: 'pl-card', title: queue?.playlist ?? '', attrs: { role: 'button', tabindex: '0' }, onclick: () => navigate('remote-playlist') },
      h('div', { class: 'pl-art' },
        cover(queue?.playlist || device.name, false, 160, queuePicture(queue)),
        h('span', { class: 'badge' }, icon('monitor', 11), h('span', { text: device.name }))),
      h('div', { class: 'pl-title ellipsis', text: queue?.playlist || t('remotePlaylist.eyebrow') }),
      h('div', { class: 'pl-sub ellipsis', text: queue ? `${t('remotePlaylist.eyebrow')} · ${t('remotePlaylist.files', queue.items.length)}` : t('remotePlaylist.waiting') }));
    card.addEventListener('keydown', event => {
      if (event.key === 'Enter' && event.target === card) navigate('remote-playlist');
    });
    remoteSlot.replaceChildren(
      h('div', { class: 'section-head' }, h('b', { text: t('remotePlaylist.on', device.name) })),
      h('div', { class: 'pgrid' }, card));
  };
  const count = h('span', { class: 'muted' });
  const grid = h('div', { class: 'pgrid' });
  const none = h('p', { class: 'none' });

  function fill(): void {
    chipButtons.forEach(button => button.classList.toggle('on', button.dataset.chip === chip));
    const all = library.playlists;
    const shown = filterByTitle(all, query).filter(IN_CHIP[chip]).sort(SORTS[sort]);
    grid.replaceChildren(...shown.map(libraryCard));
    count.textContent = t('player.library.count', shown.length, all.length);
    none.hidden = shown.length > 0;
    none.textContent = query.trim() ? t('player.library.none', query.trim()) : t('player.library.noneFilter');
    refreshResume();
  }

  // The card only asks for what is on it: the position moves it several times
  // a second, and rebuilding every thumbnail with it would be pointless.
  let meter: HTMLElement | null = null;
  let time: HTMLElement | null = null;
  function refreshResume(): void {
    meter = null;
    time = null;
    const now = ui.nowPlaying;
    const found = now ? findBlock(now.blockKey) : null;
    if (!now || !found || query.trim() || chip !== 'all') {
      resumeSlot.replaceChildren();
      return;
    }
    time = h('span');
    meter = h('div', { class: 'meter' }, h('i'));
    const card = h('div', { class: 'resume', attrs: { role: 'button', tabindex: '0' }, onclick: () => navigate('now-playing') },
      cover(now.track.title, found.block.video, 64, { kind: 'thumbnail', id: now.track.id }),
      h('div', { class: 'grow min0' },
        h('div', { class: 'eyebrow', text: t(ui.playing ? 'home.resume.playing' : 'home.resume.last') }),
        h('b', { class: 'ellipsis', text: now.track.title }),
        h('div', { class: 'muted ellipsis' }, `${found.playlist.title} · ${found.block.format.toUpperCase()} · `, time),
        meter),
      h('button', { class: 'round-play', title: t(ui.playing ? 'player.pause' : 'player.play'), onclick: event => {
        event.stopPropagation();
        player.togglePlay();
      } }, icon(ui.playing ? 'pause' : 'play', 20, true)));
    card.style.setProperty('--h', String(hueOf(found.playlist.title)));
    card.addEventListener('keydown', event => {
      if (event.key === 'Enter' && event.target === card) navigate('now-playing');
    });
    resumeSlot.replaceChildren(card);
    patchResume();
  }
  function patchResume(): void {
    if (!time || !meter) return;
    time.textContent = t('player.library.position', formatTime(ui.position), formatTime(ui.duration));
    meter.style.setProperty('--p', String(ui.duration > 0 ? Math.min(1, ui.position / ui.duration) : 0));
  }

  let shownKey = '';
  const playingKey = () => `${ui.nowPlaying?.blockKey}|${ui.nowPlaying?.track.id}|${ui.playing}`;
  onLeave(subscribe('player', () => {
    if (playingKey() === shownKey) { patchResume(); return; }
    shownKey = playingKey();
    fill();
  }));
  onLeave(subscribe('library', fill));
  onLeave(subscribe('connect', fillRemote));
  shownKey = playingKey();
  fillRemote();
  fill();

  return h('div', { class: 'screen' },
    h('div', { class: 'screen-head' }, titleRow('player', t('player.title'))),
    h('div', { class: 'toolbar' },
      h('div', { class: 'search-box' }, icon('search', 15), search),
      ...chipButtons,
      sortSelect),
    resumeSlot,
    remoteSlot,
    h('div', { class: 'section-head' },
      h('b', { text: t('player.playlists') }),
      count),
    grid,
    none);
}

/** One card per playlist, whatever its formats: which one to open is on the card. */
function libraryCard(playlist: Playlist): HTMLElement {
  const first = playlist.formats[0] as FormatBlock;
  const playable = firstPlayable(playlist);
  const device = playlist.formats.find(block => block.device)?.device;
  const offline = isOffline(playlist);
  const sounding = ui.playing && playlist.formats.some(block => block.key === ui.nowPlaying?.blockKey);
  const onlyVideo = playlist.formats.every(block => block.video);

  const open = (block: FormatBlock) => navigate('player', { b: block.key });
  const sub = offline && device ? t('player.library.offline', device.deviceName)
    : offline ? t('manager.status.offline')
    : !playlist.formats.some(block => isPlayable(block.format)) ? t('player.unplayable', first.format.toUpperCase())
    : playlist.formats.length === 1 ? `${first.format.toUpperCase()} · ${filesText(onDisk(first.tracks).length)}`
    : t('player.library.formats', playlist.formats.length);
  const added = newest(playlist);

  const card = h('div', {
    class: `pl-card${playable ? '' : ' off'}${sounding ? ' sounding' : ''}`,
    title: playlist.title,
    attrs: { role: 'button', tabindex: '0' },
    onclick: () => open(first),
  },
    h('div', { class: 'pl-art' },
      cover(playlist.title, onlyVideo, 160, playlistPicture(playlist)),
      device ? h('span', { class: 'badge' }, icon('device', 11), h('span', { text: device.deviceName }))
        : onlyVideo ? h('span', { class: 'badge' }, icon('video', 11), h('span', { text: t('player.library.video') })) : null,
      playable ? h('button', { class: 'round-play card-play', title: t(sounding ? 'player.pause' : 'player.play'), onclick: event => {
        event.stopPropagation();
        if (playlist.formats.some(block => block.key === ui.nowPlaying?.blockKey)) player.togglePlay();
        else player.playAll(playable.key);
      } }, icon(sounding ? 'pause' : 'play', 20, true)) : null),
    h('div', { class: 'pl-title ellipsis', text: playlist.title }),
    h('div', { class: `pl-sub ellipsis${offline ? ' warn-text' : ''}`, text: sub }),
    playlist.formats.length > 1 ? h('div', { class: 'fmts' }, ...playlist.formats.map(block => h('button', {
      class: 'fmt',
      text: `${block.format.toUpperCase()} · ${onDisk(block.tracks).length}`,
      onclick: event => {
        event.stopPropagation();
        open(block);
      },
    }))) : null,
    Number.isFinite(added) ? h('div', { class: 'pl-sub ellipsis', text: t('player.library.added', relative(added)) }) : null);
  card.addEventListener('keydown', event => {
    if (event.key === 'Enter' && event.target === card) open(first);
  });
  return card;
}

// === ONE PLAYLIST ===

type StateFilter = 'all' | 'ok' | 'pending' | 'off';

const IN_STATE: Record<StateFilter, (track: Track) => boolean> = {
  all: () => true,
  ok: track => track.status === 'ok',
  pending: track => track.status === 'pending' || track.status === 'failed',
  off: track => track.status === 'unavailable',
};

// The same mark on a file and on the filter that finds it. Not on disk yet is yellow, failed or not: both come down with the next sync.
const MARK: Record<Exclude<StateFilter, 'all'>, { icon: IconName; color: string }> = {
  ok: { icon: 'ok', color: 'mark-ok' },
  pending: { icon: 'pending', color: 'mark-warning' },
  off: { icon: 'close', color: 'mark-failed' },
};

function mark(filter: Exclude<StateFilter, 'all'>, name: IconName = MARK[filter].icon): HTMLElement {
  return h('span', { class: `state-mark ${MARK[filter].color}` }, icon(name, 15));
}

/** What goes on the right of a file: when it arrived, or its state and why it is not there. */
function stateCell(track: Track): HTMLElement {
  if (track.status === 'ok') return h('span', { class: 'd', text: relative(track.addedDaysAgo) });
  const text = statusText(track);
  if (track.status === 'pending') return h('span', { class: 'st', title: text }, mark('pending'), h('span', { class: 'pill warn', text }));
  if (track.status === 'failed') return h('span', { class: 'st', title: t('files.failedHint') }, mark('pending', 'warning'), h('span', { class: 'pill warn', text }));
  return h('span', { class: 'st', title: text }, mark('off'), h('span', { class: 'pill off ellipsis', text }));
}

const isStateFilter = (value: string | undefined): value is Exclude<StateFilter, 'all'> => value === 'ok' || value === 'pending' || value === 'off';

function detailView(key: string, params: Params): HTMLElement {
  const opened = findBlock(key);
  if (!opened) return libraryView();
  let { playlist, block } = opened;
  let filter = '';
  let stateFilter: StateFilter = isStateFilter(params.f) ? params.f : 'all';
  let known = new Set(onDisk(block.tracks).map(track => track.id));
  let landed = new Set<string>();
  const from: Params = params.from ? { from: params.from } : {};

  const back = () => (params.from === 'manager' ? navigate('manager', { open: playlist.id, focus: block.key }) : navigate('player'));
  const backButton = (extra = '') => h('button', { class: `icon-button${extra}`, title: t('player.back'), onclick: back }, icon('back', 18));
  const playButton = (big: boolean) => h('button', { class: `round-play${big ? ' big' : ''}`, onclick: () => {
    if (ui.nowPlaying?.blockKey === block.key) player.togglePlay();
    else player.playAll(block.key);
  } });
  const heroPlay = playButton(true);
  const miniPlay = playButton(false);
  const shuffle = h('button', { class: 'icon-button', title: t('player.shuffle'), onclick: () => player.toggleShuffle() }, icon('shuffle', 20));

  const sub = h('div', { class: 'pl-sub' });
  const switcher = h('div', { class: 'segmented' });
  const search = h('input', { class: 'input track-search', attrs: { type: 'search', placeholder: t('songs.searchPlaceholder'), 'aria-label': t('player.search') } });
  search.addEventListener('input', () => {
    filter = search.value;
    fillTracks();
  });
  const closeSearch = () => {
    search.classList.remove('open');
    search.value = '';
    filter = '';
    fillTracks();
  };
  const searchToggle = h('button', { class: 'icon-button', title: t('player.search'), onclick: () => {
    if (search.classList.contains('open')) closeSearch();
    else {
      search.classList.add('open');
      search.focus();
    }
  } }, icon('search', 18));
  const more = h('button', { class: 'icon-button', title: t('manager.menu.more') }, icon('more', 18));
  more.addEventListener('click', () => openMenu(more, [
    { icon: 'folder-open', label: t('manager.openFolder'), run: () => openFolder(playlist, block) },
    { icon: 'image', label: t('manager.menu.cover'), run: () => void changeCover(playlist) },
    { icon: 'manager', label: t('player.seeInManager'), run: seeInManager },
  ]));
  function seeInManager(): void {
    navigate('manager', { open: playlist.id, focus: block.key });
  }

  const notice = h('div', { class: 'notice' });
  const tracks = h('div', { class: 'tracks' });
  const none = h('p', { class: 'none' });
  // The states of the files (what «Ver archivos» used to show) as filters, and the sync of this playlist: no need to go to the Gestor.
  const chips = h('div', { class: 'state-chips' });
  const syncSlot = h('div', { class: 'sync-slot' });
  const liveRow = h('div', { class: 'sync-live' });
  const missingCount = () => countBy(block.tracks, 'pending') + countBy(block.tracks, 'failed');
  // The headers sort the list; the arrow marks the active one and its direction.
  const COLUMNS: { key: SortKey; class: string; label: string }[] = [
    { key: 'number', class: 'n', label: '#' },
    { key: 'title', class: '', label: t('player.column.title') },
    { key: 'added', class: 'added', label: t('player.column.added') },
  ];
  const headers = new Map<SortKey, HTMLElement>();
  for (const column of COLUMNS) {
    headers.set(column.key, h('button', {
      class: `col-sort ${column.class}`.trim(),
      attrs: { type: 'button' },
      onclick: () => {
        setOrder(block.key, nextOrder(orderOf(block.key), column.key));
        fillTracks();
        player.reorderQueue(block.key);
      },
    }, h('span', { text: column.label }), icon('chevron-down', 12)));
  }
  const cols = h('div', { class: `cols${block.video ? ' wide' : ''}` },
    headers.get('number')!,
    h('span', { class: 'thumb-col' }),
    headers.get('title')!,
    headers.get('added')!);
  const refreshHeaders = () => {
    const order = orderOf(block.key);
    for (const column of COLUMNS) {
      const header = headers.get(column.key)!;
      const active = column.key === order.key;
      header.classList.toggle('active', active);
      header.dataset.dir = active ? order.dir : '';
      header.setAttribute('aria-sort', active ? (order.dir === 'asc' ? 'ascending' : 'descending') : 'none');
      header.title = t('player.sort.by', column.label);
    }
  };

  const refreshPlay = () => {
    const sounding = ui.nowPlaying?.blockKey === block.key && ui.playing;
    const can = canPlayBlock(block);
    for (const [button, iconSize] of [[heroPlay, 24], [miniPlay, 16]] as const) {
      button.replaceChildren(icon(sounding ? 'pause' : 'play', iconSize, true));
      button.title = t(sounding ? 'player.pause' : 'player.play');
      button.setAttribute('aria-label', button.title);
      button.disabled = !can;
    }
    shuffle.disabled = !can;
    shuffle.classList.toggle('on', ui.shuffle);
  };

  function fillTracks(): void {
    refreshHeaders();
    const order = orderOf(block.key);
    let shown: Track[] = sortTracks(filterByTitle(block.tracks.filter(IN_STATE[stateFilter]), filter), order);
    // By number with no filter: what is on disk first, then what is missing (each group keeps the playlist's order).
    if (stateFilter === 'all' && order.key === 'number' && order.dir === 'asc') shown = groupByStatus(shown);
    tracks.replaceChildren(...shown.map(track =>
      trackRow(block, track, stateCell(track), { landed: landed.has(track.id), thumbWidth: 68, thumbHeight: 40 })));
    none.hidden = shown.length > 0;
    none.textContent = filter.trim() ? t('player.search.none', filter.trim()) : t(block.tracks.length ? 'files.none' : 'player.detail.none');
  }

  function refreshChips(): void {
    const counts: Record<StateFilter, number> = {
      all: block.tracks.length,
      ok: block.tracks.filter(IN_STATE.ok).length,
      pending: block.tracks.filter(IN_STATE.pending).length,
      off: block.tracks.filter(IN_STATE.off).length,
    };
    // A filter left with nothing to show goes back to all of them.
    if (counts[stateFilter] === 0) stateFilter = 'all';
    const shown: StateFilter[] = ['all'];
    if (counts.ok > 0 && counts.ok < counts.all) shown.push('ok');
    if (counts.pending > 0) shown.push('pending');
    if (counts.off > 0) shown.push('off');
    // A playlist with every file on disk has nothing to filter by.
    chips.hidden = shown.length === 1;
    chips.replaceChildren(...shown.map(state => h('button', {
      class: `fchip${stateFilter === state ? ' on' : ''}`,
      onclick: () => {
        stateFilter = state;
        refreshChips();
        fillTracks();
      },
    }, state === 'all' ? null : mark(state), h('span', { text: t(`files.filter.${state}`) }), h('small', { text: String(counts[state]) }))));
  }

  let syncShape = '';
  const currentSyncShape = () => {
    const task = runningFor(block.key);
    return `${block.key}|${task ? `live|${task.id}|${task.step}` : queuedFor(block.key) ? 'queued' : 'idle'}|${canDownload(tools.report)}|${missingCount()}`;
  };

  /** Sincronizar next to play: while it runs, its progress and Detener; queued, «Quitar de la cola». */
  function refreshSync(): void {
    syncShape = currentSyncShape();
    const task = runningFor(block.key);
    liveRow.replaceChildren();
    liveRow.hidden = !task;
    if (task) {
      syncSlot.replaceChildren(
        liveChip(task, block.format),
        h('button', { class: 'secondary-button small-button danger-text', onclick: () => api.stopDownload(task.id) },
          icon('stopped', 14, true), h('span', { text: t('tasks.stop') })));
      liveRow.replaceChildren(liveBar(task));
      return;
    }
    const queued = queuedFor(block.key);
    if (queued) {
      syncSlot.replaceChildren(
        h('span', { class: 'pm-badge queued', text: t('tasks.queue.badge') }),
        h('button', { class: 'secondary-button small-button', onclick: () => api.removeQueued(queued.id) },
          icon('close', 14), h('span', { text: t('tasks.queue.remove') })));
      return;
    }
    const missing = missingCount();
    if (missing === 0) {
      syncSlot.replaceChildren();
      return;
    }
    const sync = h('button', { class: 'sync-head', title: missing === 1 ? t('files.missing.one') : t('files.missing.many', missing), attrs: { 'aria-label': `${t('manager.sync')} (${missing})` }, onclick: () => {
      if (canDownload(tools.report)) void api.startDownload({ kind: 'update', blockKey: block.key });
    } }, icon('sync', 16), h('span', { text: t('manager.sync') }), h('span', { class: 'n', text: String(missing) }));
    sync.disabled = !canDownload(tools.report) || block.accessible === false;
    syncSlot.replaceChildren(sync);
  }

  function refreshInfo(): void {
    const newestHere = Math.min(Infinity, ...onDisk(block.tracks).map(track => track.addedDaysAgo));
    sub.textContent = [
      block.format.toUpperCase(),
      filesText(onDisk(block.tracks).length),
      missingCount() ? t('files.missingShort', missingCount()) : '',
      Number.isFinite(newestHere) ? t('player.library.added', relative(newestHere)) : '',
      block.device ? t('player.detail.on', block.device.deviceName) : '',
    ].filter(Boolean).join(' · ');
    switcher.hidden = playlist.formats.length < 2;
    switcher.replaceChildren(...playlist.formats.map(other => h('button', {
      class: other.key === block.key ? 'on' : '',
      text: other.format.toUpperCase(),
      onclick: () => navigate('player', { b: other.key, ...from }),
    })));
    if (block.accessible === false) {
      notice.replaceChildren(icon('warning', 16), h('span', { text: t('files.offline', block.device ? block.device.deviceName : playlist.folder) }));
      notice.hidden = false;
    } else if (!isPlayable(block.format)) {
      notice.replaceChildren(icon('warning', 16), h('span', { text: t('files.unplayable', block.format.toUpperCase()) }));
      notice.hidden = false;
    } else {
      notice.hidden = true;
    }
  }

  let shownKey = '';
  const playingKey = () => `${ui.nowPlaying?.blockKey}|${ui.nowPlaying?.track.id}|${ui.playing}|${ui.shuffle}`;
  onLeave(subscribe('player', () => {
    if (playingKey() === shownKey) return;
    shownKey = playingKey();
    refreshPlay();
    patchPlaying(tracks, block);
  }));
  onLeave(subscribe('library', () => {
    const found = findBlock(key);
    if (!found) {
      back();
      return;
    }
    ({ playlist, block } = found);
    // Each file that lands while this is open flashes once, to see which one it was.
    const now = onDisk(block.tracks).map(track => track.id);
    landed = new Set(now.filter(id => !known.has(id)));
    known = new Set(now);
    refreshInfo();
    refreshChips();
    refreshPlay();
    refreshSync();
    fillTracks();
    landed.clear();
  }));
  onLeave(subscribe('tasks', () => {
    if (currentSyncShape() === syncShape) {
      patchLive(syncSlot);
      patchLive(liveRow);
    } else {
      refreshSync();
    }
  }));
  shownKey = playingKey();
  refreshInfo();
  refreshChips();
  refreshPlay();
  refreshSync();
  fillTracks();

  const onKey = (event: KeyboardEvent) => {
    if (event.key !== 'Escape' || document.querySelector('#overlay-root')?.childElementCount) return;
    if (search.classList.contains('open')) closeSearch();
    else back();
  };
  window.addEventListener('keydown', onKey);
  onLeave(() => window.removeEventListener('keydown', onKey));

  // The big cover is editable: a pencil over it on hover, like the Gestor's rows.
  const heroCover = h('span', {
    class: 'hero-cover',
    title: t('manager.menu.cover'),
    attrs: { role: 'button', tabindex: '0', 'aria-label': t('manager.menu.cover') },
  },
    cover(playlist.title, block.video, 168, playlistPicture(playlist)),
    h('span', { class: 'shade' }, icon('pencil', 28)));
  heroCover.addEventListener('click', () => void changeCover(playlist));
  heroCover.addEventListener('keydown', event => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    void changeCover(playlist);
  });

  const hero = h('div', { class: 'ambient' },
    backButton(' back'),
    h('div', { class: 'hero' },
      heroCover,
      h('div', { class: 'grow min0' },
        h('div', { class: 'eyebrow', text: t(block.video ? 'player.kind.video' : 'player.kind.audio') }),
        h('h1', { text: playlist.title, title: playlist.title }),
        sub)),
    h('div', { class: 'actions' },
      heroPlay, shuffle, syncSlot, switcher,
      h('div', { class: 'spacer' }),
      search, searchToggle, more),
    liveRow);

  const scroller = h('div', { class: 'scroller' }, hero, notice, chips, cols, tracks, none);
  const detail = h('div', { class: 'detail' },
    h('div', { class: 'mini' },
      miniPlay,
      cover(playlist.title, block.video, 36, playlistPicture(playlist)),
      h('b', { class: 'ellipsis grow', text: playlist.title }),
      h('span', { class: 'fmt', text: block.format.toUpperCase() }),
      backButton()),
    scroller);
  detail.style.setProperty('--h', String(hueOf(playlist.title)));
  scroller.addEventListener('scroll', () => detail.classList.toggle('stuck', scroller.scrollTop > hero.offsetHeight - 56));

  return h('div', { class: 'screen detail-screen' }, detail);
}
