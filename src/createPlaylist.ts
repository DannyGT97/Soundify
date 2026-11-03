// What «Crear Playlist» checks before asking YouTube, and what it says once
// the playlist is registered (the Java `NewPlaylistController.doCreate`).

import type { CreatedPlaylist } from './api/types';
import { t } from './i18n';

/**
 * Worth asking YouTube about as a playlist. Looser than `isPlaylistUrl` on
 * purpose, as the Java check was: "watch?v=X&list=Y" is accepted too, and
 * the back keeps only its list.
 */
export function canCreateFrom(url: string): boolean {
  return /list=|\/playlist/.test(url);
}

/** The notice of a registered playlist: a warning when only the generic title could be had. */
export function createdNotice(created: CreatedPlaylist): { tone: 'success' | 'warning'; text: string } {
  const count = String(created.count);
  const head = created.genericTitle
    ? t('newPlaylist.registeredGenericName', created.title, count)
    : t('newPlaylist.registered', created.title, count);
  const mode = t(created.numberMode.numberFiles ? 'newPlaylist.numbered' : 'newPlaylist.notNumbered');
  // Said whenever the checkbox did not decide it: ignoring it silently would
  // look like a bug.
  const note = created.numberMode.source === 'sameFormat' ? t('newPlaylist.numberMode.sameFormat', mode)
    : created.numberMode.source === 'otherFormat' ? t('newPlaylist.numberMode.otherFormat', mode)
    : '';
  return { tone: created.genericTitle ? 'warning' : 'success', text: note ? `${head} ${note}` : head };
}
