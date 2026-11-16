import { h } from '../dom';
import { t } from '../i18n';
import { goTo } from '../overlays/tasks';
import { subscribeWhile, ui } from '../state';
import { percent } from '../tasks';

/**
 * Under the title of «Descarga por URL»: a line about the download that was ordered FROM THAT SCREEN, with the way to the
 * Gestor. A sync started in the Gestor does not show here (the ring in the title says that one, without taking a row).
 */
export function downloadStrip(): HTMLElement {
  const text = h('span', { class: 'download-strip-text' });
  const own = () => ui.running.find(running => running.origin === 'url') ?? null;
  const go = h('button', { class: 'text-button download-strip-go', text: t('strip.go'), onclick: () => { const task = own(); if (task) goTo(task); } });
  const strip = h('div', { class: 'download-strip', attrs: { role: 'status' } }, h('span', { class: 'download-strip-dot' }), text, h('span', { class: 'grow' }), go);
  const paint = () => {
    const task = own();
    strip.hidden = !task;
    if (!task) return;
    const pct = percent(task);
    text.textContent = task.step === 'retry' ? t('tasks.pill.retry')
      : pct === null ? t('tasks.pill.preparing') : t('tasks.pill.downloading', pct);
  };
  subscribeWhile(strip, 'tasks', paint);
  paint();
  return strip;
}
