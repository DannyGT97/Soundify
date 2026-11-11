import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { closeOverlay, openOverlay } from '../components/overlay';
import { navigate } from '../router';
import { subscribe, ui, type RunningTask, type TaskStatus } from '../state';
import { api } from '../api';
import { findBlock } from '../library';
import { fileNumber, markSeen, originText, percent, statusMark, summaryText } from '../tasks';
import { openActivityOverlay } from './activity';

let historyVisible = true;

/** "Tareas en segundo plano": what runs now, with its steps, and what already finished. */
export function openTasksOverlay(): void {
  markSeen();
  api.setTasksWindowOpen(true);
  const body = h('div', { class: 'stack' });
  const refresh = () => body.replaceChildren(...content());
  const unsubscribe = subscribe('tasks', refresh);
  refresh();
  openOverlay(t('tasks.title'), [body], { onClose: () => {
    unsubscribe();
    api.setTasksWindowOpen(false);
  } });
}

function content(): Node[] {
  const nodes: Node[] = [h('div', { class: 'section-label', text: t('tasks.running') })];
  if (ui.running.length > 1) {
    nodes.push(h('div', { class: 'row' }, h('div', { class: 'grow' }),
      h('button', { class: 'text-button danger-text', text: t('tasks.stopAll'), onclick: () => api.stopDownload() })));
  }
  for (const task of ui.running) {
    const pct = percent(task);
    const step = (key: string, name: string) => {
      const order = ['prepare', 'download', 'retry'];
      const state = order.indexOf(name) < order.indexOf(task.step) ? 'done' : name === task.step ? 'current' : '';
      return h('span', { class: `tasks-step ${state}`, text: t(key) });
    };
    const bar = h('div', { class: `progress${pct === null ? ' indeterminate' : ''}` }, h('div', { class: 'progress-fill' }));
    if (pct !== null) (bar.firstChild as HTMLElement).style.width = `${pct}%`;
    nodes.push(h('div', { class: 'card stack-12', attrs: { 'data-running': String(task.id) } },
      h('div', {}, h('div', { class: 'card-title', text: task.title }), h('div', { class: 'subtitle', text: originText(task.origin) })),
      h('div', { class: 'row gap-6' }, step('tasks.step.prepare', 'prepare'), step('tasks.step.download', 'download'), step('tasks.step.retry', 'retry')),
      bar,
      h('div', { class: 'row-title', text: progressText(task, pct) }),
      h('div', { class: 'row wrap gap-8' },
        h('button', { class: 'secondary-button danger-text', text: t('tasks.stop'), onclick: () => api.stopDownload(task.id) }),
        h('button', { class: 'secondary-button', text: t('tasks.goTo'), onclick: () => {
          closeOverlay();
          goTo(task);
        } }),
        h('button', { class: 'secondary-button', text: t('activity.open'), onclick: openActivityOverlay }))));
  }
  if (!ui.running.length) nodes.push(h('p', { class: 'help', text: t('tasks.idle') }));

  if (ui.queue.length) {
    nodes.push(h('div', { class: 'row center gap-8' },
      h('div', { class: 'section-label', text: t('tasks.queue', ui.queue.length) }),
      h('div', { class: 'grow' }),
      h('button', { class: 'text-button', text: t('tasks.queue.clear'), onclick: api.clearQueue })));
    nodes.push(h('div', { class: 'history-list', attrs: { 'data-queue': '' } }, ...ui.queue.map((item, index) => h('div', { class: 'history-row', attrs: { 'data-queued': String(item.id) } },
      h('span', { class: 'subtitle queue-position', text: String(index + 1) }),
      h('div', { class: 'grow min0' },
        h('div', { class: 'row-title ellipsis', text: item.title }),
        h('div', { class: 'subtitle', text: originText(item.origin) })),
      h('button', { class: 'icon-button', title: t('tasks.queue.remove'), attrs: { 'aria-label': t('tasks.queue.remove') }, onclick: () => api.removeQueued(item.id) }, icon('close', 16))))));
  }

  const toggle = h('button', { class: 'text-button', text: historyVisible ? t('tasks.history.hide') : t('tasks.history.show') });
  toggle.addEventListener('click', () => {
    historyVisible = !historyVisible;
    toggle.closest('.stack')?.replaceChildren(...content());
  });
  nodes.push(h('div', { class: 'row center gap-8' },
    h('div', { class: 'section-label', text: t('tasks.history', ui.history.length) }),
    h('div', { class: 'grow' }),
    ui.history.length ? h('button', { class: 'text-button', text: t('tasks.history.clear'), onclick: api.clearTaskHistory }) : null,
    toggle));
  if (historyVisible) {
    nodes.push(h('div', { class: 'history-list' }, ...ui.history.map(item => h('div', { class: 'history-row' },
      markOf(item.status),
      h('div', { class: 'grow min0' },
        h('div', { class: 'row-title ellipsis', text: item.title }),
        h('div', { class: 'subtitle', text: `${originText(item.origin)} · ${t(`tasks.status.${item.status}`)}` }),
        item.summary ? h('div', { class: 'help', text: summaryText(item) }) : null),
      h('span', { class: 'subtitle', text: item.finishedAt })))));
  }
  return nodes;
}

function progressText(task: RunningTask, pct: number | null): string {
  if (task.step === 'retry') return t('tasks.pill.retry');
  return pct === null ? t('tasks.pill.preparing') : t('tasks.progress', fileNumber(task), task.total);
}

function markOf(status: TaskStatus): HTMLElement {
  const mark = statusMark(status);
  return h('span', { class: `tasks-mark mark-${status}` }, icon(mark.icon, mark.filled ? 14 : 18, mark.filled));
}

/**
 * A playlist's download is shown where it lives: the Gestor, with its row
 * unfolded and the block in view. A loose URL has no block there, so it goes
 * back to the download screen.
 */
function goTo(task: RunningTask): void {
  const found = findBlock(task.blockKey);
  if (found) navigate('manager', { open: found.playlist.id, focus: found.block.key });
  else navigate(task.origin === 'manager' ? 'manager' : 'download');
}
