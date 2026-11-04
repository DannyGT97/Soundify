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
  if (onAndroid) document.documentElement.dataset.platform = 'android';
  matchMedia(PHONE_QUERY).addEventListener('change', () => emit('player-placement'));
}
