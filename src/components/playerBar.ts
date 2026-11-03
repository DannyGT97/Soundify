// The player bar at the bottom of every screen. Visible only while something
// is loaded; its ✕ stops and unloads, which hides it again.

import { h } from '../dom';
import { t } from '../i18n';
import { findBlock } from '../library';
import * as player from '../player';
import { navigate } from '../router';
import { subscribeWhile, ui } from '../state';
import { cover } from './cover';
import { buildTransport, buildVolume, control } from './transport';

export function buildPlayerBar(): HTMLElement {
  const coverBox = h('div', { class: 'player-cover' });
  const title = h('div', { class: 'row-title ellipsis' });
  // The playlist's name opens it in the Reproductor; the rest of the block, Reproduciendo.
  const meta = h('div', { class: 'subtitle ellipsis player-link', onclick: (event: MouseEvent) => {
    event.stopPropagation();
    if (ui.nowPlaying) navigate('player', { b: ui.nowPlaying.blockKey });
  } });
  const now = h('div', { class: 'player-now', onclick: () => navigate('now-playing') }, coverBox,
    h('div', { class: 'player-now-text' }, title, meta));

  const bar = h('footer', { class: 'player-bar' },
    now,
    buildTransport(),
    h('div', { class: 'player-right' },
      buildVolume(),
      control('close', 'player.close', player.close)));

  let shownKey = '';
  const refresh = () => {
    const np = ui.nowPlaying;
    bar.hidden = !np || !['bottom', 'both'].includes(ui.playerPlacement);
    if (!np) return;
    const found = findBlock(np.blockKey);
    title.textContent = np.track.title;
    meta.textContent = found ? `${found.playlist.title} · ${found.block.format}` : '';
    meta.title = found ? t('nowPlaying.openPlaylist') : '';
    const key = `${np.blockKey}/${np.track.id}`;
    if (key !== shownKey) {
      shownKey = key;
      coverBox.replaceChildren(cover(np.track.title, found?.block.video ?? false, 48, { kind: 'thumbnail', id: np.track.id }));
    }
  };
  subscribeWhile(bar, 'player', refresh);
  subscribeWhile(bar, 'player-placement', refresh);
  refresh();
  return bar;
}
