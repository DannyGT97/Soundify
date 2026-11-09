// Where the app is running, for what CSS cannot decide alone. Android is the
// device (what the back can do there); a phone is the room on screen (a window
// that narrow gets the same layout in the browser, which is how it is tested).

import { emit } from './state';

const PHONE_QUERY = '(max-width: 700px), (max-height: 500px)';

/** `?mockAndroid` shows in the browser what the APK hides. */
export const onAndroid: boolean =
  (typeof navigator !== 'undefined' && /android/i.test(navigator.userAgent)) ||
  (typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockAndroid'));

/** Must match the media query of `styles/phone.css`. */
export function isPhone(): boolean {
  return typeof matchMedia !== 'undefined' && matchMedia(PHONE_QUERY).matches;
}

/** Turning the phone or resizing the window can change what the player's places show. */
export function installPlatform(): void {
  if (onAndroid) {
    document.documentElement.dataset.platform = 'android';
    watchKeyboard();
  }
  matchMedia(PHONE_QUERY).addEventListener('change', () => emit('player-placement'));
}

// === KEYBOARD ===

const NON_TEXT = new Set(['checkbox', 'radio', 'range', 'button', 'submit', 'reset', 'file', 'color', 'image']);

/** A field one types in (not a checkbox, a slider or a button). */
export function typesText(el: Element | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  if (el instanceof HTMLTextAreaElement) return !el.readOnly;
  if (el instanceof HTMLInputElement) return !NON_TEXT.has(el.type) && !el.readOnly;
  return el.isContentEditable;
}

/** The keyboard is open when the page lost this much height while a text field has the focus. */
export function keyboardOpen(typing: boolean, height: number, tallest: number): boolean {
  return typing && height < tallest - 120;
}

/**
 * The on-screen keyboard shrinks the page (MainActivity pads it), and on a tablet turned sideways
 * what is left was the menu, the player, the summary of the form and a sliver of the field. While
 * it is open `data-typing` hides all of that (`phone.css`) and the field stays in view.
 */
function watchKeyboard(): void {
  const root = document.documentElement;
  let tallest = window.innerHeight;
  let width = window.innerWidth;
  const update = () => {
    if (window.innerWidth !== width) { width = window.innerWidth; tallest = window.innerHeight; }
    tallest = Math.max(tallest, window.innerHeight);
    const open = keyboardOpen(typesText(document.activeElement), window.innerHeight, tallest);
    if (open === ('typing' in root.dataset)) return;
    if (open) root.dataset.typing = '';
    else delete root.dataset.typing;
    if (open) requestAnimationFrame(() => document.activeElement?.scrollIntoView({ block: 'center' }));
  };
  window.addEventListener('resize', update);
  document.addEventListener('focusin', update);
  document.addEventListener('focusout', () => setTimeout(update, 0));
}
