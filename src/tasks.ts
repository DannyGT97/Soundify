// PLACEHOLDER TASK: a download that only advances a counter, so the menu's
// indicator and the tasks window can be seen in every state. The real one
// will be fed by the yt-dlp process (its --print hooks and progress lines).

import { t } from './i18n';
import { emit, ui, type TaskStatus } from './state';

let timer: number | undefined;

export function startTask(title: string, origin: string, total: number): void {
  if (ui.task) return;
  ui.task = { title, origin, step: 'prepare', done: 0, total };
  ui.unseenResult = null;
  emit('tasks');
  let ticks = 0;
  timer = window.setInterval(() => {
    const task = ui.task;
    if (!task) return;
    ticks++;
    if (task.step === 'prepare') {
      if (ticks >= 3) task.step = 'download';
    } else if (task.done < task.total) {
      task.done++;
    } else {
      finish('ok');
      return;
    }
    emit('tasks');
  }, 700);
}

export function stopTask(): void {
  if (ui.task) finish('stopped');
}

export function clearHistory(): void {
  ui.history = [];
  emit('tasks');
}

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

function finish(status: TaskStatus): void {
  window.clearInterval(timer);
  const task = ui.task;
  if (!task) return;
  const now = new Date();
  const time = `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`;
  ui.history.unshift({ title: task.title, origin: task.origin, status, finishedAt: time });
  ui.history = ui.history.slice(0, 20);
  ui.task = null;
  ui.unseenResult = status;
  emit('tasks');
}
