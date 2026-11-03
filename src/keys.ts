// The space bar plays and pauses, wherever the focus is — except where it
// types. Left to the browser, a space "clicked" again whatever button had
// the focus: the last one pressed, so it reopened «Cambiar portada» or
// «Ver archivos» instead of pausing.

import { togglePlay } from './player';
import { ui } from './state';

/** What a space would type into rather than press: text fields, lists and editable areas. */
const KEYS_THAT_TYPE = new Set(['text', 'search', 'url', 'email', 'password', 'number', 'tel']);

export interface FocusLike { tagName: string; type?: string; isContentEditable?: boolean }

export function spaceTypes(target: FocusLike | null): boolean {
  if (!target) return false;
  if (target.isContentEditable) return true;
  switch (target.tagName) {
    case 'TEXTAREA':
    case 'SELECT':
      return true;
    case 'INPUT':
      return KEYS_THAT_TYPE.has((target.type ?? 'text').toLowerCase());
    default:
      return false;
  }
}

export function installSpaceToPlay(): void {
  const isSpace = (event: KeyboardEvent) => event.key === ' ' && !event.ctrlKey && !event.altKey && !event.metaKey;
  // Capture: before the focused element sees it. A button "clicks" on keyup,
  // so that one is swallowed too.
  window.addEventListener('keydown', event => {
    if (!isSpace(event) || spaceTypes(event.target as FocusLike | null)) return;
    event.preventDefault();
    event.stopPropagation();
    if (!event.repeat && ui.nowPlaying) togglePlay();
  }, true);
  window.addEventListener('keyup', event => {
    if (isSpace(event) && !spaceTypes(event.target as FocusLike | null)) event.preventDefault();
  }, true);
}
