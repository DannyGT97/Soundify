// Windows that open ON TOP of the app (dark veil + panel), like OverlayHost in
// the Java app: they close with ✕, Escape or a click outside the panel.

import { h } from '../dom';
import { t } from '../i18n';
import { icon } from '../icons';

let current: { scrim: HTMLElement; onClose?: () => void } | null = null;

export interface OverlayOptions {
  wide?: boolean;
  onClose?: () => void;
}

/** Opens `body` under a header with `title`. Returns the function that closes it. */
export function openOverlay(title: string | Node, body: Node[], options: OverlayOptions = {}): () => void {
  closeOverlay();
  const heading = typeof title === 'string' ? h('h2', { class: 'overlay-title', text: title }) : title;
  const panel = h('div', { class: `overlay-window${options.wide ? ' overlay-wide' : ''}`, attrs: { role: 'dialog' } },
    h('div', { class: 'overlay-header' },
      heading,
      h('button', { class: 'icon-button', title: t('common.close'), onclick: () => closeOverlay() }, icon('close'))),
    ...body);
  const scrim = h('div', { class: 'overlay-scrim' }, panel);
  scrim.addEventListener('mousedown', e => {
    if (e.target === scrim) closeOverlay();
  });
  document.getElementById('overlay-root')?.append(scrim);
  current = { scrim, onClose: options.onClose };
  return closeOverlay;
}

export function closeOverlay(): void {
  if (!current) return;
  const { scrim, onClose } = current;
  current = null;
  scrim.classList.add('overlay-leaving');
  window.setTimeout(() => scrim.remove(), 150);
  onClose?.();
}

window.addEventListener('keydown', e => {
  if (e.key === 'Escape') closeOverlay();
});
