import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';

export type ToastTone = 'success' | 'warning' | 'error' | 'neutral';
export interface ToastOptions { tone: ToastTone; text: string; }
let root: HTMLElement | null = null;

export function mountToasts(host: HTMLElement): void {
  root = h('div', { class: 'toast-stack', attrs: { 'aria-live': 'polite', 'aria-atomic': 'false' } });
  host.append(root);
}

/** Add a temporary notice to the app shell so navigation never removes it. */
export function notify({ tone, text }: ToastOptions): void {
  if (!root) return;
  const symbol = tone === 'success' ? 'ok' : tone === 'warning' ? 'warning' : tone === 'error' ? 'error' : 'info';
  const toast = h('div', { class: `toast toast-${tone}`, attrs: { role: tone === 'error' ? 'alert' : 'status' } },
    icon(symbol, 18), h('span', { class: 'toast-text', text }),
    h('button', { class: 'toast-close', title: t('common.close'), onclick: () => dismiss() }, icon('close', 16)));
  let timer = window.setTimeout(dismiss, 5000);
  function dismiss(): void {
    window.clearTimeout(timer);
    toast.classList.add('toast-leaving');
    window.setTimeout(() => toast.remove(), 180);
  }
  root.append(toast);
}
