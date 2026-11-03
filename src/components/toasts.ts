import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';

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
let root: HTMLElement | null = null;

export function mountToasts(host: HTMLElement): void {
  root = h('div', { class: 'toast-stack', attrs: { 'aria-live': 'polite', 'aria-atomic': 'false' } });
  host.append(root);
}

/** Add a notice to the app shell so navigation never removes it. Returns how to take it away. */
export function notify(options: ToastOptions): () => void {
  return show(options).dismiss;
}

/** A notice kept until taken away whose text changes meanwhile (a percentage). */
export function liveNotice(tone: ToastTone, text: string, action?: ToastAction): { setText(text: string): void; dismiss(): void } {
  return show({ tone, text, persistent: true, action });
}

function show({ tone, text, persistent = false, action }: ToastOptions): { setText(text: string): void; dismiss(): void } {
  if (!root) return { setText: () => {}, dismiss: () => {} };
  const symbol = tone === 'success' ? 'ok' : tone === 'warning' ? 'warning' : tone === 'error' ? 'error' : 'info';
  const body = h('span', { class: 'toast-text', text });
  const toast = h('div', { class: `toast toast-${tone}`, attrs: { role: tone === 'error' ? 'alert' : 'status' } },
    icon(symbol, 18), body,
    action ? h('button', { class: 'toast-action', text: action.label, onclick: () => { dismiss(); action.run(); } }) : null,
    h('button', { class: 'toast-close', title: t('common.close'), onclick: () => dismiss() }, icon('close', 16)));
  const timer = persistent ? undefined : window.setTimeout(dismiss, 5000);
  function dismiss(): void {
    window.clearTimeout(timer);
    toast.classList.add('toast-leaving');
    window.setTimeout(() => toast.remove(), 180);
  }
  root.append(toast);
  return { setText: next => { body.textContent = next; }, dismiss };
}
