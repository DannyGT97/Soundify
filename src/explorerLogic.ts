// The explorer's pure logic (paths, order, filter, sizes, dates): the window itself is overlays/explorer.ts.

import { normalizeForSearch } from './search';

/** What an entry is, for its icon and for what a file chooser accepts. */
export type EntryKind = 'folder' | 'image' | 'audio' | 'video' | 'text' | 'file';
export type SortBy = 'name' | 'date' | 'size';

/** The listing's shape for both a computer's folder and a phone's. */
export interface ExplorerEntry {
  name: string;
  isFolder: boolean;
  sizeBytes: number;
  /** Local time `2026-11-17T09:30:00`, when the system says. */
  modified?: string | null;
  /** A folder with a Soundify manifest. */
  playlist?: { title: string; format: string } | null;
}

export const IMAGE_EXTENSIONS = ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp'];
const AUDIO_EXTENSIONS = ['mp3', 'm4a', 'flac', 'wav', 'ogg', 'opus', 'aac', 'wma', 'alac'];
const VIDEO_EXTENSIONS = ['mp4', 'mkv', 'webm', 'avi', 'mov', 'flv'];

export function extensionOf(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

export function kindOf(entry: Pick<ExplorerEntry, 'name' | 'isFolder'>): EntryKind {
  if (entry.isFolder) return 'folder';
  const extension = extensionOf(entry.name);
  if (IMAGE_EXTENSIONS.includes(extension)) return 'image';
  if (AUDIO_EXTENSIONS.includes(extension)) return 'audio';
  if (VIDEO_EXTENSIONS.includes(extension)) return 'video';
  if (extension === 'txt') return 'text';
  return 'file';
}

// === PATHS OF THIS COMPUTER ===

/** `C:\` or `/`: where a path starts, and what is left after it. */
export function splitPath(path: string): { root: string; parts: string[] } {
  const drive = /^[A-Za-z]:[\\/]?/.exec(path);
  const root = drive ? `${drive[0].slice(0, 2)}\\` : path.startsWith('/') ? '/' : '';
  const rest = path.slice(drive ? drive[0].length : root.length);
  return { root, parts: rest.split(/[\\/]+/).filter(Boolean) };
}

function separatorOf(path: string): string {
  return /^[A-Za-z]:/.test(path) || path.includes('\\') ? '\\' : '/';
}

export function joinPath(parent: string, name: string): string {
  const separator = separatorOf(parent);
  return parent.endsWith('\\') || parent.endsWith('/') ? `${parent}${name}` : `${parent}${separator}${name}`;
}

/** The folder above, or null at a drive's (or the disk's) root. */
export function parentPath(path: string): string | null {
  const { root, parts } = splitPath(path);
  if (parts.length === 0) return null;
  const up = parts.slice(0, -1);
  return up.length ? joinPath(root, up.join(separatorOf(path))) : root;
}

export interface Crumb {
  label: string;
  path: string;
}

/** Every step from the root to `path`, each with the path it leads to. */
export function crumbsOf(path: string): Crumb[] {
  const { root, parts } = splitPath(path);
  const separator = separatorOf(path);
  const crumbs: Crumb[] = [];
  if (root) crumbs.push({ label: root === '/' ? '/' : root.slice(0, 2), path: root });
  let acc = root;
  for (const part of parts) {
    acc = acc.endsWith('\\') || acc.endsWith('/') || acc === '' ? `${acc}${part}` : `${acc}${separator}${part}`;
    crumbs.push({ label: part, path: acc });
  }
  return crumbs;
}

/** «C:» for a drive's path, null for anything else. */
export function driveLetter(path: string): string | null {
  const match = /^([A-Za-z]):[\\/]?$/.exec(path);
  return match ? `${(match[1] as string).toUpperCase()}:` : null;
}

/** A path compared the way its system does: Windows ignores capitals and takes either slash. */
function normalized(path: string): string {
  const windows = /^[A-Za-z]:/.test(path) || path.includes('\\');
  const flat = windows ? path.replace(/\//g, '\\').toLowerCase() : path;
  return flat.length > 1 ? flat.replace(/[\\/]+$/, '') : flat;
}

export function samePath(a: string, b: string): boolean {
  return normalized(a) === normalized(b);
}

/** Is `path` the folder `root` or something inside it? */
export function isInside(path: string, root: string): boolean {
  const inner = normalized(path);
  const outer = normalized(root);
  if (inner === outer) return true;
  if (outer === '/') return inner.startsWith('/');
  // `C:\` keeps its slash once normalized to `c:`: the next character decides.
  return inner.startsWith(`${outer}${/^[a-z]:/.test(outer) ? '\\' : '/'}`);
}

/** Of these disks, the one `path` is on (the deepest, if one sits inside another). */
export function driveFor<T extends { path: string }>(path: string, drives: readonly T[]): T | undefined {
  return drives.filter(drive => isInside(path, drive.path)).sort((a, b) => b.path.length - a.path.length)[0];
}

/** `path` at the front of a list of recent folders: once, and no more than `max`. */
export function remember(list: readonly string[], path: string, max: number): string[] {
  return [path, ...list.filter(item => !samePath(item, path))].slice(0, max);
}

/** `path` starred, or no longer if it was. */
export function toggled(list: readonly string[], path: string): string[] {
  return list.some(item => samePath(item, path)) ? list.filter(item => !samePath(item, path)) : [...list, path];
}

// === THE LIST ===

/** Folders first, each group in the order asked; "10" after "9" (numeric names). */
export function sortEntries<T extends ExplorerEntry>(entries: readonly T[], by: SortBy): T[] {
  const byName = (a: T, b: T) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });
  const compare = (a: T, b: T): number => {
    if (by === 'date') return (b.modified ?? '').localeCompare(a.modified ?? '') || byName(a, b);
    if (by === 'size') return b.sizeBytes - a.sizeBytes || byName(a, b);
    return byName(a, b);
  };
  const folders = entries.filter(entry => entry.isFolder).sort(by === 'size' ? byName : compare);
  const files = entries.filter(entry => !entry.isFolder).sort(compare);
  return [...folders, ...files];
}

/** What the search box leaves: names with no accents or capitals; a playlist also by its title. */
export function filterEntries<T extends ExplorerEntry>(entries: readonly T[], query: string): T[] {
  const needle = normalizeForSearch(query.trim());
  if (!needle) return [...entries];
  return entries.filter(entry => normalizeForSearch(`${entry.name} ${entry.playlist?.title ?? ''}`).includes(needle));
}

export function sizeText(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(bytes >= 100 * 1024 ** 3 ? 0 : 1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(bytes >= 100 * 1024 ** 2 ? 0 : 1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return bytes > 0 ? `${bytes} B` : '';
}

/** Whole days between `iso` and `now` (0 = today), or null when the date is missing or unreadable. */
export function daysSince(iso: string | null | undefined, now: Date): number | null {
  if (!iso) return null;
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return null;
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  return Math.max(0, Math.round((day(now) - day(then)) / 86_400_000));
}

/** A folder name worth typing: one folder, nothing Windows refuses. */
export function validFolderName(name: string): boolean {
  const trimmed = name.trim();
  return trimmed !== '' && trimmed !== '.' && trimmed !== '..' && !/[\\/:*?"<>|]/.test(trimmed) && !trimmed.endsWith('.');
}

/** Does `name` already exist in `entries` (the way Windows compares: ignoring capitals)? */
export function hasName(entries: readonly ExplorerEntry[], name: string): boolean {
  const lower = name.trim().toLowerCase();
  return entries.some(entry => entry.name.toLowerCase() === lower);
}

/** "Title (format)": the folder Soundify gives a playlist. */
export function playlistFolderName(title: string, format: string): string {
  return `${title} (${format.toLowerCase()})`;
}

/** The title and format a phone's folder name says it is, or null. */
export function playlistFromName(name: string): { title: string; format: string } | null {
  const match = /^(.+) \(([a-z0-9]{2,5})\)$/i.exec(name);
  return match ? { title: match[1] as string, format: (match[2] as string).toUpperCase() } : null;
}
