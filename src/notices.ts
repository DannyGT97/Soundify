// The notices that came up (a download finished, something failed, an import was done), kept to read later:
// a toast goes away by itself after a few seconds, this list does not. Only in memory, the last `MAX_NOTICES`.

import { emit, ui } from './state';

export type NoticeTone = 'success' | 'warning' | 'error' | 'info';
export interface Notice { id: number; tone: NoticeTone; text: string; at: Date; read: boolean; }

export const MAX_NOTICES = 50;
let nextId = 1;

export function addNotice(tone: NoticeTone, text: string, at: Date = new Date()): void {
  ui.notices.unshift({ id: nextId++, tone, text, at, read: false });
  if (ui.notices.length > MAX_NOTICES) ui.notices.length = MAX_NOTICES;
  emit('notices');
}

export function unreadNotices(list: readonly Notice[] = ui.notices): number {
  return list.filter(notice => !notice.read).length;
}

export function markNoticesRead(): void {
  if (!unreadNotices()) return;
  for (const notice of ui.notices) notice.read = true;
  emit('notices');
}

export function clearNotices(): void {
  ui.notices.length = 0;
  emit('notices');
}

/** «14:05», how the list says when. */
export function noticeTime(at: Date): string {
  return `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
}
