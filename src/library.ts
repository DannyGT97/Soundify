// The library as last loaded from the back. Views read it synchronously;
// `loadLibrary()` refreshes it and emits `library` so open screens redraw.

import { api, type FormatBlock, type FormatCatalog, type Playlist, type Status, type Track } from './api';
import { t } from './i18n';
import { emit } from './state';

export const library = {
  playlists: [] as Playlist[],
  formats: { audio: [], video: [] } as FormatCatalog,
};

export async function loadLibrary(): Promise<void> {
  const [playlists, formats] = await Promise.all([api.listPlaylists(), api.formats()]);
  library.playlists = playlists;
  library.formats = formats;
  emit('library');
}

let reloadTimer: ReturnType<typeof setTimeout> | undefined;

/**
 * A reload a moment later, folding a burst into one: a playlist can bring
 * down several files a second, and each one asks for the whole library.
 */
export function loadLibrarySoon(delay = 400): void {
  if (reloadTimer !== undefined) return;
  reloadTimer = setTimeout(() => {
    reloadTimer = undefined;
    void loadLibrary();
  }, delay);
}

export function findBlock(key: string | null | undefined,
                          playlists: Playlist[] = library.playlists): { playlist: Playlist; block: FormatBlock } | null {
  if (!key) return null;
  for (const playlist of playlists) {
    const found = playlist.formats.find(b => b.key === key);
    if (found) return { playlist, block: found };
  }
  return null;
}

export function countBy(list: Track[], status: Status): number {
  return list.filter(tr => tr.status === status).length;
}

/**
 * The status as "Ver archivos" shows it. An unavailable file says why right
 * there: with the reason only in a tooltip you had to go file by file to
 * understand why they were missing. The generic reason adds nothing, so it is left out.
 */
export function statusText(track: Track): string {
  if (track.status === 'unavailable' && track.reason && track.reason !== 'unavailable.generic') {
    return t('songs.status.unavailableWithReason', t(track.reason));
  }
  return t(`songs.status.${track.status}`);
}

/** Files actually on disk: what the player lists and plays. */
export function onDisk(list: Track[]): Track[] {
  return list.filter(tr => tr.status === 'ok');
}

/** The order «Agrupar por estado» shows: what is already on disk first. */
export const STATUS_ORDER: readonly Status[] = ['ok', 'pending', 'failed', 'unavailable'];

/** The files grouped by state in that order, each group keeping the playlist's order. */
export function groupByStatus(list: readonly Track[]): Track[] {
  return [...list].sort((a, b) => STATUS_ORDER.indexOf(a.status) - STATUS_ORDER.indexOf(b.status));
}

export function filesText(n: number): string {
  return t(n === 1 ? 'common.files.one' : 'common.files.many', n);
}

export function pendingText(n: number): string {
  return t(n === 1 ? 'common.pending.one' : 'common.pending.many', n);
}
