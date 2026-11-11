// The running download as the rows show it: a chip, a bar and the numbers that
// change with every percent, written in place so nothing has to be rebuilt.

import { h } from '../dom';
import type { RunningTask } from '../state';
import { ui } from '../state';
import { liveText, percent, progressLine } from '../tasks';

const SWEEP_MS = 2600;

/** What changes with every percent, written over the rows already built. */
export function patchLive(root: HTMLElement): void {
  root.querySelectorAll<HTMLElement>('[data-live]').forEach(el => {
    // Each piece says whose it is: with two downloads at once they do not share numbers.
    const task = ui.running.find(running => String(running.id) === el.dataset.task);
    if (!task) return;
    const pct = percent(task);
    switch (el.dataset.live) {
      case 'chip': el.textContent = liveText(task, el.dataset.format ?? ''); break;
      case 'line': el.textContent = progressLine(task); break;
      case 'pct': el.textContent = percentText(task); break;
      case 'bar': if (pct !== null) el.style.width = `${pct}%`; break;
    }
  });
}

/** «Descargando MP3 · 3/12», with a dot that breathes. */
export function liveChip(task: RunningTask, format: string): HTMLElement {
  const text = h('span', { text: liveText(task, format), attrs: { 'data-live': 'chip', 'data-format': format, 'data-task': String(task.id) } });
  const chip = h('span', { class: 'pm-live' }, h('span', { class: 'pm-live-dot' }), text);
  chip.style.setProperty('--live-phase', livePhase());
  return chip;
}

export function liveBar(task: RunningTask, extraClass = ''): HTMLElement {
  const pct = percent(task);
  const fill = h('div', { class: 'progress-fill', attrs: { 'data-live': 'bar', 'data-task': String(task.id) } });
  if (pct !== null) fill.style.width = `${pct}%`;
  return h('div', { class: `progress${pct === null ? ' indeterminate' : ''}${extraClass}` }, fill);
}

export function percentText(task: RunningTask): string {
  const pct = percent(task);
  return pct === null ? '' : `${pct}%`;
}

/** A negative delay that puts every live animation at the same point of the same clock. */
export function livePhase(): string {
  return `-${Date.now() % SWEEP_MS}ms`;
}
