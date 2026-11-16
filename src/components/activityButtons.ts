import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { unreadNotices } from '../notices';
import { openTasksOverlay } from '../overlays/tasks';
import { emit, subscribe, subscribeWhile, ui, type TaskStatus } from '../state';
import { overallPercent, percent, statusMark } from '../tasks';

/** How long the ring keeps the result of the task that just ended (a mark), before it goes away. */
export const FLASH_MS = 4500;

let flash: TaskStatus | null = null;
let flashTimer: number | undefined;

/**
 * The result of a task that has just ended stays in the ring for a few seconds and then it goes: the ring is for
 * what is happening, not a place where «Terminada» stays for ever (what happened is in the list of the bell).
 * Started once, from the shell.
 */
export function watchActivity(): void {
  let wasRunning = ui.running.length > 0;
  subscribe('tasks', () => {
    const running = ui.running.length > 0;
    if (running) {
      window.clearTimeout(flashTimer);
      flash = null;
    } else if (wasRunning) {
      flash = ui.unseenResult ?? ui.history[0]?.status ?? null;
      window.clearTimeout(flashTimer);
      flashTimer = window.setTimeout(() => {
        flash = null;
        emit('tasks');
      }, FLASH_MS);
    }
    wasRunning = running;
  });
}

/** The result being shown in the ring, if any (for the tests). */
export function flashedResult(): TaskStatus | null {
  return flash;
}

const NS = 'http://www.w3.org/2000/svg';

/** A ring that fills as the download goes; spinning while it cannot tell how far it is. */
function progressRing(pct: number | null): SVGSVGElement {
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 26 26');
  svg.setAttribute('width', '30');
  svg.setAttribute('height', '30');
  const radius = 10;
  const length = 2 * Math.PI * radius;
  for (const cls of ['ring-track', 'ring-arc']) {
    const circle = document.createElementNS(NS, 'circle');
    circle.setAttribute('cx', '13');
    circle.setAttribute('cy', '13');
    circle.setAttribute('r', String(radius));
    circle.setAttribute('class', cls);
    if (cls === 'ring-arc') circle.setAttribute('stroke-dasharray', `${length * (pct === null ? 0.25 : pct / 100)} ${length}`);
    svg.append(circle);
  }
  if (pct === null) svg.classList.add('ring-spin');
  return svg;
}

/** What the ring says, in words: for the screen readers, the tooltip and the tests. */
function ringText(): { title: string; sub: string } {
  const task = ui.task;
  if (task) {
    const several = ui.running.length > 1;
    const pct = several ? overallPercent(ui.running) : percent();
    const title = several ? t('tasks.pill.several', ui.running.length, pct ?? 0)
      : task.step === 'retry' ? t('tasks.pill.retry')
      : pct === null ? t('tasks.pill.preparing') : t('tasks.pill.downloading', pct);
    const names = ui.running.map(running => running.title).join(' · ');
    return { title, sub: ui.queue.length ? `${names} · ${t('tasks.pill.more', ui.queue.length)}` : names };
  }
  return { title: flash ? t(`tasks.pill.${flash}`) : '', sub: ui.history[0]?.title ?? '' };
}

/**
 * The two buttons at the end of every screen's title: the ring with the progress of what downloads (only while
 * something does, and a few seconds after it ends) and the bell with the list of tasks and notices. Both open the
 * same window.
 */
export function activityButtons(): HTMLElement {
  const ring = h('button', { class: 'icon-button activity-ring', onclick: openTasksOverlay });
  const bellDot = h('span', { class: 'activity-dot', attrs: { 'aria-hidden': 'true' } });
  const bell = h('button', { class: 'icon-button activity-bell', onclick: openTasksOverlay }, icon('bell', 20), bellDot);
  const tools = h('span', { class: 'title-tools' }, ring, bell);

  const paintRing = () => {
    const running = ui.task !== null;
    ring.hidden = !running && !flash;
    if (ring.hidden) return;
    const { title, sub } = ringText();
    ring.title = title;
    ring.setAttribute('aria-label', title);
    ring.classList.toggle('flash', !running);
    if (running) {
      const several = ui.running.length > 1;
      const pct = several ? overallPercent(ui.running) : percent();
      ring.replaceChildren(progressRing(pct), ...(pct === null ? [] : [h('span', { class: 'activity-ring-pct', text: String(pct) })]),
        h('span', { class: 'activity-ring-text sr-only', text: title }), h('span', { class: 'activity-ring-sub sr-only', text: sub }));
    } else if (flash) {
      const mark = statusMark(flash);
      ring.replaceChildren(h('span', { class: `mark-${flash} activity-ring-mark` }, icon(mark.icon, 20, mark.filled)),
        h('span', { class: 'activity-ring-text sr-only', text: title }), h('span', { class: 'activity-ring-sub sr-only', text: sub }));
    }
  };
  const paintBell = () => {
    const unread = unreadNotices();
    bellDot.hidden = unread === 0;
    const label = unread ? t('activity.bell.unread', unread) : t('activity.bell');
    bell.title = label;
    bell.setAttribute('aria-label', label);
  };
  subscribeWhile(tools, 'tasks', () => { paintRing(); paintBell(); });
  subscribeWhile(tools, 'notices', paintBell);
  paintRing();
  paintBell();
  return tools;
}
