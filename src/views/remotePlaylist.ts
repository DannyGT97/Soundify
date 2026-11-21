// The playlist of the device being controlled (Soundify Connect): what it is playing, listed here so a song can be
// picked from this device. Technically a «remote playlist»: the files are on that device, and it says which one.
// When it plays nothing (or the user wants another playlist) this lists ITS library instead, to pick a playlist, see its files
// and start one: nothing has to sound on that device first.

import type { ConnectLibraryEntry, ConnectQueue, ConnectQueueItem } from '../api';
import { cover } from '../components/cover';
import { browseRemote, connect, playRemoteFrom, playRemoteTrack, showRemoteLibrary } from '../connect';
import { hueOf, queuePicture, showPicture } from '../covers';
import { h } from '../dom';
import { t } from '../i18n';
import { remoteGuide } from '../guides';
import { icon } from '../icons';
import { showGuide } from '../overlays/guide';
import * as player from '../player';
import { navigate, onLeave } from '../router';
import { filterByTitle } from '../search';
import { subscribe, ui } from '../state';
import { ago } from '../relativeTime';
import { nextOrder, orderOf, setOrder, sortTracks, type SortKey } from '../trackOrder';

/** A file of the remote playlist with its place in it (the number the other device gives it). */
type Numbered = ConnectQueueItem & { number: number; addedDaysAgo: number };

type StateFilter = 'all' | 'ok' | 'pending' | 'off';

/** The same states and filters as a local playlist (a remote one only lists what is not on disk when it was browsed). */
const IN_STATE: Record<StateFilter, (item: ConnectQueueItem) => boolean> = {
  all: () => true,
  ok: item => !item.status,
  pending: item => item.status === 'pending' || item.status === 'failed',
  off: item => item.status === 'unavailable',
};

function relative(days: number): string {
  const { key, amount } = ago(days);
  return t(key, amount);
}

/** What goes on the right of a file: when it was downloaded there, or why it is not on that device. */
function stateCell(item: ConnectQueueItem & { addedDaysAgo: number }): HTMLElement {
  if (!item.status) return h('span', { class: 'd', text: item.addedDaysAgo >= 0 && item.addedDaysAgo < 100000 ? relative(item.addedDaysAgo) : '' });
  const text = t(item.status === 'unavailable' ? 'files.filter.off' : 'files.filter.pending');
  return h('span', { class: 'st', title: text }, h('span', { class: `pill ${item.status === 'unavailable' ? 'off' : 'warn'} ellipsis`, text }));
}

function numberCell(current: boolean, number: number): HTMLElement {
  return current
    ? h('span', { class: `eq${ui.playing ? '' : ' paused'}` }, h('i'), h('i'), h('i'))
    : h('span', { class: 'num', text: String(number) });
}

function stateOf(item: ConnectQueueItem): 'playing' | 'paused' | 'idle' {
  if (ui.nowPlaying?.track.id !== item.videoId) return 'idle';
  return ui.playing ? 'playing' : 'paused';
}

/** The files that are listed: the playlist browsed, or the one that plays (null while a browsed one is loading, or in the library). */
function shownQueue(): ConnectQueue | null {
  if (connect.browsed) return connect.browsed.queue;
  return connect.pickingLibrary ? null : connect.queue;
}

/** The list of playlists of the device's library, rather than the files of one. */
function libraryMode(): boolean {
  return !connect.browsed && (connect.pickingLibrary || !connect.queue);
}

/** Picking a file: the playlist browsed starts from it, the one that plays goes to it. */
function pick(videoId: string): void {
  if (connect.browsed) playRemoteFrom(connect.browsed.key, videoId);
  else playRemoteTrack(videoId);
}

/** The cells of the big picture of the library: one per playlist, up to four (the layout follows how many: `[data-n]` in the CSS). */
function mosaicCells(entries: ConnectLibraryEntry[]): HTMLElement[] {
  const shown = entries.slice(0, 4);
  return shown.map((entry, index) => {
    const tile = cover(entry.title, entry.video, 84);
    // The tall cells (two side by side, or the big one of three) are half as wide as they are high: the picture is cut for that.
    const tall = shown.length === 2 || (shown.length === 3 && index === 0);
    if (entry.cover) showPicture(tile, { kind: 'thumbnail', id: entry.cover }, tall ? 0.5 : 1);
    return tile;
  });
}

function libraryRow(entry: ConnectLibraryEntry): HTMLElement {
  const tile = cover(entry.title, entry.video, 40);
  // Its picture: the thumbnail of its first file (this device fetches it if it does not have it, like in every list).
  if (entry.cover) showPicture(tile, { kind: 'thumbnail', id: entry.cover });
  const element = h('div', { class: 'track lib-row', attrs: { role: 'button', tabindex: '0', 'data-library': entry.key } },
    tile,
    h('span', { class: 't row-title ellipsis', text: entry.title }),
    h('span', { class: 'd', text: `${entry.format} · ${t('remotePlaylist.files', entry.count)}` }));
  element.addEventListener('click', () => browseRemote(entry));
  element.addEventListener('keydown', event => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    browseRemote(entry);
  });
  return element;
}

function row(item: ConnectQueueItem & { addedDaysAgo: number }, number: number): HTMLElement {
  const state = stateOf(item);
  const current = state !== 'idle';
  // Listed but not on that device: it cannot be played from here.
  const off = !!item.status;
  // The thumbnail of the video, by its id (the same one the local lists use: it is fetched if this device does not have it).
  const thumb = cover(item.title, false, 40);
  showPicture(thumb, { kind: 'thumbnail', id: item.videoId });
  const element = h('div', { class: `track track-row${current ? ' playing' : ''}${off ? ' off' : ''}`, attrs: { role: 'button', tabindex: off ? '-1' : '0' } },
    h('span', { class: 'n' }, numberCell(current, number), h('span', { class: 'hover-play' }, icon(state === 'playing' ? 'pause' : 'play', 14, true))),
    thumb,
    h('span', { class: 't row-title ellipsis', text: item.title }),
    stateCell(item));
  element.dataset.id = item.videoId;
  element.dataset.number = String(number);
  element.dataset.state = state;
  if (!off) {
    element.addEventListener('click', () => pick(item.videoId));
    element.addEventListener('keydown', event => {
      if (event.key !== 'Enter') return;
      event.preventDefault();
      pick(item.videoId);
    });
  }
  return element;
}

export function remotePlaylistView(): HTMLElement {
  const device = ui.remote;
  // Nothing is being controlled: there is no remote playlist to show.
  if (!device) {
    queueMicrotask(() => navigate('player'));
    return h('div', { class: 'screen' });
  }
  // The first time, it is said that this is another device's music, not the user's own.
  queueMicrotask(() => showGuide(remoteGuide(device.name)));
  const back = () => navigate('now-playing');
  const backButton = (extra = '') => h('button', { class: `icon-button${extra}`, title: t('player.back'), onclick: back }, icon('back', 18));
  const heading = h('h1');
  const eyebrow = h('div', { class: 'eyebrow' });
  const sub = h('div', { class: 'pl-sub' });
  const tracks = h('div', { class: 'tracks' });
  const none = h('p', { class: 'none' });
  let filter = '';
  // The same sorting as a local playlist (# and title; it has no download date). It only changes how it is listed
  // here: the other device plays its own order.
  const orderKey = () => `remote:${device.deviceId}:${shownQueue()?.playlist ?? connect.browsed?.title ?? ''}`;
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
        setOrder(orderKey(), nextOrder(orderOf(orderKey()), column.key));
        fillTracks();
      },
    }, h('span', { text: column.label }), icon('chevron-down', 12)));
  }
  const cols = h('div', { class: 'cols' }, headers.get('number')!, h('span', { class: 'thumb-col' }), headers.get('title')!, headers.get('added')!);
  // The states of the files as filters (shown only when the playlist has files in more than one state).
  const chips = h('div', { class: 'state-chips' });
  let stateFilter: StateFilter = 'all';
  const refreshChips = () => {
    const all = shownQueue()?.items ?? [];
    const counts: Record<StateFilter, number> = { all: all.length, ok: all.filter(IN_STATE.ok).length, pending: all.filter(IN_STATE.pending).length, off: all.filter(IN_STATE.off).length };
    if (counts[stateFilter] === 0) stateFilter = 'all';
    const shown: StateFilter[] = ['all'];
    if (counts.ok > 0 && counts.ok < counts.all) shown.push('ok');
    if (counts.pending > 0) shown.push('pending');
    if (counts.off > 0) shown.push('off');
    chips.hidden = libraryMode() || shown.length === 1;
    chips.replaceChildren(...shown.map(state => h('button', {
      class: `fchip${stateFilter === state ? ' on' : ''}`,
      onclick: () => {
        stateFilter = state;
        fillTracks();
      },
    }, h('span', { text: t(`files.filter.${state}`) }), h('small', { text: String(counts[state]) }))));
  };
  const refreshHeaders = () => {
    const order = orderOf(orderKey());
    for (const column of COLUMNS) {
      const header = headers.get(column.key)!;
      const active = column.key === order.key;
      header.classList.toggle('active', active);
      header.dataset.dir = active ? order.dir : '';
      header.setAttribute('aria-sort', active ? (order.dir === 'asc' ? 'ascending' : 'descending') : 'none');
      header.title = t('player.sort.by', column.label);
    }
  };
  const fillTracks = () => {
    refreshHeaders();
    refreshChips();
    cols.hidden = libraryMode();
    if (libraryMode()) {
      const entries = filterByTitle(connect.library.map(entry => ({ ...entry })), filter);
      tracks.replaceChildren(...entries.map(libraryRow));
      none.hidden = entries.length > 0;
      none.textContent = !connect.library.length ? t('remotePlaylist.libraryEmpty', device.name) : t('player.search.none', filter.trim());
      return;
    }
    const items: Numbered[] = (shownQueue()?.items ?? []).map((item, index) => ({ ...item, number: index + 1, addedDaysAgo: item.addedDaysAgo ?? 100000 }));
    const shown = sortTracks(filterByTitle(items.filter(IN_STATE[stateFilter]), filter), orderOf(orderKey()));
    tracks.replaceChildren(...shown.map(item => row(item, item.number)));
    none.hidden = shown.length > 0;
    none.textContent = connect.browsed && !connect.browsed.queue ? t('remotePlaylist.loading') : !items.length ? t('remotePlaylist.empty') : t('player.search.none', filter.trim());
  };

  // Play/pause, shuffle and search, like a local playlist: they go to the device being controlled.
  const sounds = () => !!ui.nowPlaying && !!shownQueue()?.items.some(item => item.videoId === ui.nowPlaying?.track.id);
  const playButton = (big: boolean) => h('button', { class: `round-play${big ? ' big' : ''}`, onclick: () => {
    if (sounds()) player.togglePlay();
    else {
      const first = shownQueue()?.items[0];
      if (first) pick(first.videoId);
    }
  } });
  const heroPlay = playButton(true);
  const miniPlay = playButton(false);
  const shuffle = h('button', { class: 'icon-button', title: t('player.shuffle'), onclick: () => player.toggleShuffle() }, icon('shuffle', 20));
  const refreshPlay = () => {
    const playing = sounds() && ui.playing;
    for (const [button, iconSize] of [[heroPlay, 24], [miniPlay, 16]] as const) {
      button.replaceChildren(icon(playing ? 'pause' : 'play', iconSize, true));
      button.title = t(playing ? 'player.pause' : 'player.play');
      button.setAttribute('aria-label', button.title);
      button.disabled = !shownQueue()?.items.length;
      button.hidden = libraryMode();
    }
    shuffle.disabled = !shownQueue()?.items.length;
    shuffle.hidden = libraryMode();
    shuffle.classList.toggle('on', ui.shuffle);
  };
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
  const miniTitle = h('b', { class: 'ellipsis grow' });
  const heroCover = cover(device.name, false, 168);
  // The list of playlists has no picture of its own: up to four of theirs side by side (one alone fills the tile).
  const heroMosaic = h('div', { class: 'cover-mosaic', attrs: { 'aria-hidden': 'true' } });
  heroMosaic.hidden = true;
  let shownFor: object | null | undefined;

  // Between the files that are listed and the list of playlists: one button, and it says where it goes.
  const switchButton = h('button', { class: 'secondary-button remote-switch' });
  switchButton.addEventListener('click', () => {
    if (libraryMode()) showRemoteLibrary(false);
    else showRemoteLibrary(true);
  });
  const paintSwitch = () => {
    const library = libraryMode();
    // In the library: back to what plays (only if it plays a playlist). Elsewhere: to the library (only if it has any).
    switchButton.hidden = library ? !connect.queue : !connect.library.length;
    switchButton.textContent = library ? t('remotePlaylist.backToPlaying') : t('remotePlaylist.allPlaylists');
  };

  /** What is shown, to tell when it changed (the queue and the browsed playlist are replaced, never edited). */
  const shownNow = (): object | null => (libraryMode() ? connect.library : connect.browsed ?? connect.queue);

  const fill = () => {
    const queue = shownQueue();
    shownFor = shownNow();
    const library = libraryMode();
    const title = library ? t('remotePlaylist.libraryTitle') : queue?.playlist || connect.browsed?.title || t('remotePlaylist.eyebrow');
    heading.textContent = title;
    heading.title = title;
    miniTitle.textContent = title;
    detail.style.setProperty('--h', String(hueOf(title)));
    sub.textContent = [t('remotePlaylist.on', device.name), library ? t('remotePlaylist.playlists', connect.library.length) : queue ? t('remotePlaylist.files', queue.items.length) : ''].filter(Boolean).join(' · ');
    // The big picture: the playlist's, or (in the list) the first one's, so it is never an empty tile.
    const firstCover = connect.library.find(entry => entry.cover)?.cover;
    const several = library && connect.library.length > 1;
    heroMosaic.hidden = !several;
    heroCover.hidden = several;
    if (several) {
      heroMosaic.dataset.n = String(Math.min(4, connect.library.length));
      heroMosaic.replaceChildren(...mosaicCells(connect.library));
    }
    showPicture(heroCover, library ? (firstCover ? { kind: 'thumbnail', id: firstCover } : null) : queuePicture(queue));
    eyebrow.textContent = library ? t('remotePlaylist.libraryEyebrow', device.name) : t('remotePlaylist.eyebrow');
    searchToggle.hidden = false;
    paintSwitch();
    fillTracks();
    refreshPlay();
    tracks.querySelector('.playing')?.scrollIntoView({ block: 'center' });
  };

  /** What sounds changes all the time: only the rows whose state changed are touched (the list can have hundreds). */
  const paint = () => {
    refreshPlay();
    for (const element of tracks.querySelectorAll<HTMLElement>(':scope > .track-row')) {
      const item = { videoId: element.dataset.id ?? '', title: '' };
      const state = stateOf(item);
      if (element.dataset.state === state) continue;
      element.dataset.state = state;
      element.classList.toggle('playing', state !== 'idle');
      const n = element.querySelector<HTMLElement>(':scope > .n');
      n?.querySelector(':scope > .eq, :scope > .num')?.replaceWith(numberCell(state !== 'idle', Number(element.dataset.number)));
      n?.querySelector(':scope > .hover-play')?.replaceChildren(icon(state === 'playing' ? 'pause' : 'play', 14, true));
    }
  };

  const hero = h('div', { class: 'ambient' },
    backButton(' back'),
    h('div', { class: 'hero' },
      h('span', { class: 'hero-cover' }, heroCover, heroMosaic),
      h('div', { class: 'grow min0' },
        eyebrow,
        heading,
        sub)),
    h('div', { class: 'actions' },
      heroPlay, shuffle, switchButton,
      h('div', { class: 'spacer' }),
      search, searchToggle));
  const scroller = h('div', { class: 'scroller' }, hero, chips, cols, tracks, none);
  const detail = h('div', { class: 'detail' },
    h('div', { class: 'mini' }, miniPlay, icon('monitor', 18), miniTitle, h('span', { class: 'fmt', text: device.name }), backButton()),
    scroller);
  scroller.addEventListener('scroll', () => {
    detail.classList.toggle('stuck', scroller.scrollTop > hero.offsetHeight - 56);
    // The bar that slides in stops short of the scrollbar so that stays whole.
    detail.style.setProperty('--sbw', `${scroller.offsetWidth - scroller.clientWidth}px`);
  });

  onLeave(subscribe('connect', () => {
    // The device is not being controlled any more: back to the player.
    if (!ui.remote) return navigate('player');
    if (shownNow() !== shownFor) fill();
    else paintSwitch();
  }));
  onLeave(subscribe('player', paint));
  const onKey = (event: KeyboardEvent) => {
    if (event.key !== 'Escape' || document.querySelector('#overlay-root')?.childElementCount) return;
    if (search.classList.contains('open')) closeSearch();
    else back();
  };
  window.addEventListener('keydown', onKey);
  onLeave(() => window.removeEventListener('keydown', onKey));

  fill();
  return h('div', { class: 'screen detail-screen' }, detail);
}
