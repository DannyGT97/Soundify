// The order of the files of a playlist in the Reproductor: by number (#), title
// or when they were downloaded. Pure logic, plus the order each playlist was
// left in while the app is open (the queue plays in the order that is shown).

import { normalizeForSearch } from './search';

export type SortKey = 'number' | 'title' | 'added';
export type SortDir = 'asc' | 'desc';
export interface TrackOrder {
  key: SortKey;
  dir: SortDir;
}

interface Sortable {
  number: number;
  title: string;
  addedDaysAgo: number;
}

/** What the list shows until a header is pressed: by number, 1 first. */
export const DEFAULT_ORDER: TrackOrder = { key: 'number', dir: 'asc' };

/**
 * The order a header gives when it is pressed. Pressing the active one flips
 * it; pressing another starts ascending: 1 first, A first, and for «Descargado»
 * the most recent first.
 */
export function nextOrder(current: TrackOrder, key: SortKey): TrackOrder {
  if (current.key === key) return { key, dir: current.dir === 'asc' ? 'desc' : 'asc' };
  return { key, dir: 'asc' };
}

/** For "added", ascending is the most recent first: the key is how many days ago. */
function compare(a: Sortable, b: Sortable, key: SortKey): number {
  if (key === 'title') {
    return normalizeForSearch(a.title).localeCompare(normalizeForSearch(b.title), undefined, { numeric: true });
  }
  if (key === 'added') return a.addedDaysAgo - b.addedDaysAgo;
  return a.number - b.number;
}

/** A sorted copy. Ties keep the number order, so the result is always the same. */
export function sortTracks<T extends Sortable>(tracks: readonly T[], order: TrackOrder): T[] {
  const sign = order.dir === 'asc' ? 1 : -1;
  return [...tracks].sort((a, b) => sign * compare(a, b, order.key) || a.number - b.number);
}

// === PER PLAYLIST, WHILE THE APP IS OPEN ===

const orders = new Map<string, TrackOrder>();

export function orderOf(blockKey: string): TrackOrder {
  return orders.get(blockKey) ?? DEFAULT_ORDER;
}

export function setOrder(blockKey: string, order: TrackOrder): void {
  orders.set(blockKey, order);
}
