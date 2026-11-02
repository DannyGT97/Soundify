import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { closeOverlay, openOverlay } from '../components/overlay';
import { navigate } from '../router';
import { subscribe, ui } from '../state';
import { api } from '../api';
import { markSeen, originText, percent } from '../tasks';
import { openActivityOverlay } from './activity';

let historyVisible = true;

/** "Tareas en segundo plano": what runs now, with its steps, and what already finished. */
export function openTasksOverlay(): void {
  markSeen();
  const body = h('div', { class: 'stack' });
  const refresh = () => body.replaceChildren(...content());
  const unsubscribe = subscribe('tasks', refresh);
  refresh();
  openOverlay(t('tasks.title'), [body], { onClose: unsubscribe });
}

function content(): Node[] {
  const nodes: Node[] = [h('div', { class: 'section-label', text: t('tasks.running') })];
  const task = ui.task;
  if (task) {
    const pct = percent();
    const step = (key: string, name: string) => {
      const order = ['prepare', 'download', 'retry'];
      const state = order.indexOf(name) < order.indexOf(task.step) ? 'done' : name === task.step ? 'current' : '';
      return h('span', { class: `tasks-step ${state}`, text: t(key) });
    };
    const bar = h('div', { class: `progress${pct === null ? ' indeterminate' : ''}` }, h('div', { class: 'progress-fill' }));
    if (pct !== null) (bar.firstChild as HTMLElement).style.width = `${pct}%`;
    nodes.push(h('div', { class: 'card stack-12' },
      h('div', {}, h('div', { class: 'card-title', text: task.title }), h('div', { class: 'subtitle', text: originText(task.origin) })),
      h('div', { class: 'row gap-6' }, step('tasks.step.prepare', 'prepare'), step('tasks.step.download', 'download'), step('tasks.step.retry', 'retry')),
      bar,
      h('div', { class: 'row-title', text: pct === null ? t('tasks.pill.preparing') : t('tasks.progress', task.done, task.total) }),
      h('div', { class: 'row wrap gap-8' },
        h('button', { class: 'secondary-button danger-text', text: t('tasks.stop'), onclick: api.stopDownload }),
        h('button', { class: 'secondary-button', text: t('tasks.goTo'), onclick: () => {
          closeOverlay();
          navigate(task.origin === 'manager' ? 'manager' : 'download');
        } }),
        h('button', { class: 'secondary-button', text: t('activity.open'), onclick: openActivityOverlay }))));
  } else {
    nodes.push(h('p', { class: 'help', text: t('tasks.idle') }));
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
      h('span', { class: `tasks-mark ${item.status === 'ok' ? 'mark-ok' : 'mark-stopped'}` }, item.status === 'ok' ? icon('ok', 18) : icon('stopped', 14, true)),
      h('div', { class: 'grow min0' },
        h('div', { class: 'row-title ellipsis', text: item.title }),
        h('div', { class: 'subtitle', text: `${originText(item.origin)} · ${t(`tasks.status.${item.status}`)}` })),
      h('span', { class: 'subtitle', text: item.finishedAt })))));
  }
  return nodes;
}
