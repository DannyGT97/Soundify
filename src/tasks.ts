// What the front works out about the background task. Starting and stopping
// it is the back's job (`api.startDownload` / `api.stopDownload`); its state
// arrives as a `TasksViewWire` and is mapped into `ui` here.

import type { ActivityLine, DownloadNotice, TaskOrigin, TaskRecordWire, TasksViewWire } from './api/types';
import { hasKey, t } from './i18n';
import { emit, ui, type FinishedTask, type QueuedTask, type RunningTask, type TaskStatus } from './state';

const STATUSES: readonly TaskStatus[] = ['ok', 'warning', 'stopped', 'failed'];

export function markSeen(): void {
  ui.unseenResult = null;
  emit('tasks');
}

/** 0–100, or null while still preparing (the ring spins instead). */
export function percent(task: RunningTask | null = ui.task): number | null {
  if (!task || task.step === 'prepare' || task.fraction === null) return null;
  return Math.round(Math.min(1, Math.max(0, task.fraction)) * 100);
}

/** The running task that downloads this archive, if any. */
export function runningFor(blockKey: string): RunningTask | null {
  return ui.running.find(task => task.blockKey === blockKey) ?? null;
}

/** One ring for several tasks: the mean of those that already know how far they are (null if none does). */
export function overallPercent(tasks: readonly RunningTask[]): number | null {
  const known = tasks.map(task => percent(task)).filter((value): value is number => value !== null);
  if (!known.length) return null;
  return Math.round(known.reduce((sum, value) => sum + value, 0) / tasks.length);
}

/** The file being downloaded now, 1-based, for "Archivo X de Y". */
export function fileNumber(task: RunningTask): number {
  const total = Math.max(1, task.total);
  return Math.min(total, Math.floor((task.fraction ?? 0) * total) + 1);
}

/** The mark of a finished task: icon name and whether it is drawn filled. */
export function statusMark(status: TaskStatus): { icon: 'ok' | 'warning' | 'stopped' | 'error'; filled: boolean } {
  switch (status) {
    case 'ok': return { icon: 'ok', filled: false };
    case 'warning': return { icon: 'warning', filled: false };
    case 'stopped': return { icon: 'stopped', filled: true };
    case 'failed': return { icon: 'error', filled: false };
  }
}

/** The task of this archive that waits its turn, if any. */
export function queuedFor(blockKey: string): QueuedTask | null {
  return ui.queue.find(q => q.blockKey === blockKey) ?? null;
}

/** The chip of a Gestor row while one of its formats downloads: «Descargando MP3 · 3/12». */
export function liveText(task: RunningTask, format: string): string {
  switch (task.step) {
    case 'prepare': return t('manager.live.prepare', format);
    case 'retry': return t('manager.live.retry', format);
    case 'download': return t('manager.live.download', format, fileNumber(task), Math.max(1, task.total));
  }
}

/** The line over a block's bar: «Archivo 3 de 12», or «Preparando…» before the counter exists. */
export function progressLine(task: RunningTask): string {
  return task.step === 'prepare' ? t('tasks.pill.preparing') : t('tasks.progress', fileNumber(task), Math.max(1, task.total));
}

export function originText(origin: string): string {
  return origin === 'manager' ? t('tasks.origin.manager') : t('tasks.origin.url');
}

// === FROM THE BACK ===

/** The back's view, as `ui` keeps it. Pure: `applyTasksView` is the one that stores it. */
export function toTasksState(view: TasksViewWire, now: Date): Pick<typeof ui, 'task' | 'running' | 'queue' | 'unseenResult' | 'history'> {
  // One task normally, two with «Descargar más rápido»; `task` is the first of them.
  const running: RunningTask[] = view.running.map(task => ({
    id: task.id, title: task.title, origin: toOrigin(task.origin), step: task.step, fraction: task.fraction, total: task.total, blockKey: task.blockKey ?? null,
  }));
  return {
    task: running[0] ?? null,
    running,
    queue: (view.queued ?? []).map(q => ({ id: q.id, title: q.title, origin: toOrigin(q.origin), blockKey: q.blockKey ?? null })),
    unseenResult: view.running.length ? null : toStatus(view.unseen),
    history: view.history.map(record => toFinished(record, now)),
  };
}

export function applyTasksView(view: TasksViewWire): void {
  Object.assign(ui, toTasksState(view, new Date()));
  emit('tasks');
}

/**
 * "2026-11-03 18:42" (the history's stamp) as the list shows it: just the
 * time today, day and month before that.
 */
export function shortStamp(stamp: string | null | undefined, now: Date): string {
  const match = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}:\d{2})/.exec(stamp ?? '');
  if (!match) return stamp ?? '';
  const [, year, month, day, time] = match.map(part => part ?? '');
  const today = Number(year) === now.getFullYear() && Number(month) === now.getMonth() + 1 && Number(day) === now.getDate();
  return today ? `${time}` : `${day}/${month} ${time}`;
}

// === TEXTS ===

/** An app message's arguments may be keys themselves (a `failReason.*`): those are translated too. */
export function messageText(key: string, args: readonly string[]): string {
  return t(key, ...args.map(arg => hasKey(arg) ? t(arg) : arg));
}

export function activityText(line: ActivityLine): string {
  return line.kind === 'raw' ? line.text : messageText(line.key, line.args);
}

/** The result line of a finished task: a key this app wrote, or text the Java app already translated. */
export function summaryText(task: FinishedTask): string {
  if (!task.summary) return '';
  return hasKey(task.summary) ? messageText(task.summary, task.summaryArgs) : task.summary;
}

/** Notices where the fix is in Ajustes → Avanzado (cookies): they stay on screen with a way there. */
export function pointsToSettings(notice: DownloadNotice): boolean {
  return /^activity\.botcheck\.(streak|retry)Notice(NoCookies)?$/.test(notice.key);
}

export function noticeTone(notice: DownloadNotice): 'success' | 'warning' | 'error' {
  const status = toStatus(notice.status);
  return status === 'ok' ? 'success' : status === 'failed' ? 'error' : 'warning';
}

// === PRIVATE HELPERS ===

function toFinished(record: TaskRecordWire, now: Date): FinishedTask {
  return {
    title: record.title ?? '',
    origin: toOrigin(record.origin),
    status: toStatus(record.status) ?? 'ok',
    summary: record.summary ?? null,
    summaryArgs: record.summaryArgs ?? [],
    finishedAt: shortStamp(record.finishedAt, now),
  };
}

function toStatus(status: string | null | undefined): TaskStatus | null {
  const lower = status?.toLowerCase();
  return STATUSES.find(s => s === lower) ?? null;
}

/** The back sends the i18n key of the origin. */
function toOrigin(origin: string | null | undefined): TaskOrigin {
  return origin === 'tasks.origin.manager' || origin === 'manager' ? 'manager' : 'url';
}
