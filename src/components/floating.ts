// A menu that floats next to the button that opens it. Fixed and hung from the
// overlay layer, so a scrolling list or a window's edge cannot cut it off.

import { h } from '../dom';
import { icon, type IconName } from '../icons';

let close: (() => void) | null = null;

export function closeFloating(): void {
  close?.();
}

/** Below `anchor` (or above when it does not fit), kept inside the window. */
export function openFloating(anchor: HTMLElement, content: Node[], options: { alignLeft?: boolean; class?: string } = {}): void {
  closeFloating();
  const panel = h('div', { class: `floating${options.class ? ` ${options.class}` : ''}` }, ...content);
  (document.getElementById('overlay-root') ?? document.body).append(panel);
  const box = anchor.getBoundingClientRect();
  const { width, height } = panel.getBoundingClientRect();
  const left = options.alignLeft ? box.left : box.right - width;
  const top = box.bottom + 6 + height > window.innerHeight - 8 ? box.top - 6 - height : box.bottom + 6;
  panel.style.left = `${Math.max(8, Math.min(left, window.innerWidth - width - 8))}px`;
  panel.style.top = `${Math.max(8, top)}px`;

  const outside = (event: MouseEvent) => {
    if (!panel.contains(event.target as Node)) closeFloating();
  };
  const escape = (event: KeyboardEvent) => {
    if (event.key !== 'Escape') return;
    // Only the menu: the window under it stays open.
    event.stopImmediatePropagation();
    closeFloating();
  };
  close = () => {
    close = null;
    panel.remove();
    document.removeEventListener('mousedown', outside, true);
    window.removeEventListener('keydown', escape, true);
    window.removeEventListener('resize', closeFloating);
  };
  document.addEventListener('mousedown', outside, true);
  window.addEventListener('keydown', escape, true);
  window.addEventListener('resize', closeFloating);
}

export interface MenuItem {
  icon: IconName;
  label: string;
  danger?: boolean;
  run: () => void;
}

/** A list of actions; `null` draws a separator. */
export function openMenu(anchor: HTMLElement, items: (MenuItem | null)[]): void {
  openFloating(anchor, items.map(item => item
    ? h('button', { class: item.danger ? 'danger' : '', onclick: () => { closeFloating(); item.run(); } },
      icon(item.icon, 16), h('span', { text: item.label }))
    : h('hr')));
}
