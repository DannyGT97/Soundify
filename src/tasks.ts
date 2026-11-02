// What the front works out about the background task. Starting and stopping
// it is the back's job (`api.startDownload` / `api.stopDownload`).

import { t } from './i18n';
import { emit, ui } from './state';

export function markSeen(): void {
  ui.unseenResult = null;
  emit('tasks');
}

/** 0–100, or null while still preparing (the ring spins instead). */
export function percent(): number | null {
  const task = ui.task;
  if (!task || task.step === 'prepare') return null;
  return Math.round((task.done / Math.max(1, task.total)) * 100);
}

export function originText(origin: string): string {
  return origin === 'manager' ? t('tasks.origin.manager') : t('tasks.origin.url');
}
