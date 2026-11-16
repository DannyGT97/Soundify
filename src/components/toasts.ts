import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { addNotice } from '../notices';

export type ToastTone = 'success' | 'warning' | 'error' | 'neutral';
export interface ToastOptions {
  tone: ToastTone;
  text: string;
  /** Stays until closed: for something the user has to act on, not something that happened. */
  persistent?: boolean;
  /** A link at the end («Reintentar», «Ir a Ajustes»): runs and takes the notice away. */
  action?: ToastAction;
}
export interface ToastAction {
  label: string;
  run: () => void;
}

/** How long each kind stays: what failed is read slower than what went well. */
export const TOAST_MS: Record<ToastTone, number> = { success: 5000, neutral: 5000, warning: 6500, error: 8000 };
/** More than this at once would eat the page: the older ones leave the screen (they stay in the list). */
const MAX_VISIBLE = 3;
const SWIPE_PX = 60;
const LEAVE_MS = 200;

let root: HTMLElement | null = null;

/**
 * The notices live in the column of the page, ABOVE the screen: a new one makes room by pushing the screen
 * down instead of covering the header, the buttons or the player.
 */
export function mountToasts(host: HTMLElement): void {
  root = h('div', { class: 'toast-stack', attrs: { 'aria-live': 'polite', 'aria-atomic': 'false' } });
  host.prepend(root);
}

/** Show a notice, and keep it in the list behind the bell. Returns how to take it away. */
export function notify(options: ToastOptions): () => void {
  addNotice(options.tone === 'neutral' ? 'info' : options.tone, options.text);
  return show(options).dismiss;
}

/** A notice kept until taken away whose text changes meanwhile (a percentage). It is not something that happened: it is not listed. */
export function liveNotice(tone: ToastTone, text: string, action?: ToastAction): { setText(text: string): void; dismiss(): void } {
  return show({ tone, text, persistent: true, action });
}

function leave(slot: HTMLElement): void {
  if (slot.classList.contains('toast-leaving')) return;
  slot.classList.add('toast-leaving');
  window.setTimeout(() => slot.remove(), LEAVE_MS);
}

function show({ tone, text, persistent = false, action }: ToastOptions): { setText(text: string): void; dismiss(): void } {
  const stack = root;
  if (!stack) return { setText: () => {}, dismiss: () => {} };
  const symbol = tone === 'success' ? 'ok' : tone === 'warning' ? 'warning' : tone === 'error' ? 'error' : 'info';
  const body = h('span', { class: 'toast-text', text });
  const toast = h('div', { class: `toast toast-${tone}`, attrs: { role: tone === 'error' ? 'alert' : 'status' } },
    icon(symbol, 18), body,
    action ? h('button', { class: 'toast-action', text: action.label, onclick: () => { dismiss(); action.run(); } }) : null,
    h('button', { class: 'toast-close', title: t('common.close'), onclick: () => dismiss() }, icon('close', 16)));
  // The slot folds its height away on leaving, so the screen below glides back up instead of jumping.
  const slot = h('div', { class: 'toast-slot' }, toast);
  let timer: number | undefined = persistent ? undefined : window.setTimeout(dismiss, TOAST_MS[tone]);
  function dismiss(): void {
    window.clearTimeout(timer);
    leave(slot);
  }
  // Swiping it sideways takes it away.
  let startX: number | null = null;
  toast.addEventListener('pointerdown', event => {
    if ((event.target as HTMLElement).closest('button')) return;
    startX = event.clientX;
    window.clearTimeout(timer);
    toast.setPointerCapture(event.pointerId);
  });
  toast.addEventListener('pointermove', event => {
    if (startX !== null) toast.style.transform = `translateX(${event.clientX - startX}px)`;
  });
  const release = (event: PointerEvent) => {
    if (startX === null) return;
    const moved = event.clientX - startX;
    startX = null;
    if (Math.abs(moved) > SWIPE_PX) {
      toast.style.transform = `translateX(${moved > 0 ? 100 : -100}%)`;
      dismiss();
    } else {
      toast.style.transform = '';
      if (!persistent && !slot.classList.contains('toast-leaving')) timer = window.setTimeout(dismiss, TOAST_MS[tone]);
    }
  };
  toast.addEventListener('pointerup', release);
  toast.addEventListener('pointercancel', release);

  stack.prepend(slot);
  // The oldest leave the screen first; they remain in the list of notices.
  const visible = [...stack.children].filter((child): child is HTMLElement => child instanceof HTMLElement && !child.classList.contains('toast-leaving'));
  for (const old of visible.slice(MAX_VISIBLE)) leave(old);
  return { setText: next => { body.textContent = next; }, dismiss };
}
