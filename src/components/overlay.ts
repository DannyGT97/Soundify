// Windows that open ON TOP of the app (dark veil + panel), like OverlayHost in
// the Java app: they close with ✕, Escape or a click outside the panel.

import { h } from '../dom';
import { t } from '../i18n';
import { icon } from '../icons';

let current: { scrim: HTMLElement; onClose?: () => void } | null = null;
/** An entry of the browser history stands for the open window, so the phone's back gesture closes it instead of leaving the screen. */
let historyEntry = false;

export interface OverlayOptions {
  wide?: boolean;
  onClose?: () => void;
}

/** Opens `body` under a header with `title`. Returns the function that closes it. */
export function openOverlay(title: string | Node, body: Node[], options: OverlayOptions = {}): () => void {
  closeOverlay(true);
  const heading = typeof title === 'string' ? h('h2', { class: 'overlay-title', text: title }) : title;
  const panel = h('div', { class: `overlay-window${options.wide ? ' overlay-wide' : ''}`, attrs: { role: 'dialog' } },
    h('div', { class: 'overlay-header' },
      heading,
      h('button', { class: 'icon-button', title: t('common.close'), onclick: () => closeOverlay() }, icon('close'))),
    ...body);
  show(panel, options.onClose);
  return () => closeOverlay();
}

/** The same veil, closing and Escape for a window that draws all of itself. */
export function openPanel(panel: HTMLElement, onClose?: () => void): void {
  closeOverlay(true);
  panel.setAttribute('role', 'dialog');
  show(panel, onClose);
}

function show(panel: HTMLElement, onClose?: () => void): void {
  const scrim = h('div', { class: 'overlay-scrim' }, panel);
  scrim.addEventListener('mousedown', e => {
    if (e.target === scrim) closeOverlay();
  });
  document.getElementById('overlay-root')?.append(scrim);
  current = { scrim, onClose };
  if (!historyEntry) {
    try {
      history.pushState({ soundifyOverlay: true }, '');
      historyEntry = true;
    } catch {
      // Without history the window just closes the other ways.
    }
  }
}

/**
 * A header's ✕: every window with its own header closes the same way.
 */
export function closeButton(): HTMLElement {
  return h('button', { class: 'icon-button', title: t('common.close'), onclick: () => closeOverlay() }, icon('close'));
}

/** `replacing`: another window opens right now, so the history entry stays for it. */
export function closeOverlay(replacing = false): void {
  if (!current) return;
  const { scrim, onClose } = current;
  current = null;
  if (!replacing && historyEntry) {
    // Looked at a moment later: if another window opened meanwhile (a confirmation that goes on to a progress window) it
    // keeps the entry; if the screen changed (a link inside the window) the entry is no longer the current one.
    window.setTimeout(() => {
      if (current || !historyEntry) return;
      historyEntry = false;
      if ((history.state as { soundifyOverlay?: boolean } | null)?.soundifyOverlay) history.back();
    }, 0);
  }
  scrim.classList.add('overlay-leaving');
  window.setTimeout(() => scrim.remove(), 150);
  onClose?.();
}

/**
 * Closes the window and, once its history entry is gone, runs `next` (to go somewhere else): navigating while the
 * entry is still being popped would be undone by it.
 */
export function closeOverlayThen(next: () => void): void {
  if (!current || !historyEntry || !(history.state as { soundifyOverlay?: boolean } | null)?.soundifyOverlay) {
    closeOverlay();
    next();
    return;
  }
  window.addEventListener('popstate', () => next(), { once: true });
  closeOverlay();
}

// The back gesture (or button) of the phone: closes the window, and the entry it used is already gone.
window.addEventListener('popstate', () => {
  if (!current || !historyEntry) return;
  historyEntry = false;
  closeOverlay();
});

window.addEventListener('keydown', e => {
  if (e.key === 'Escape') closeOverlay();
});
