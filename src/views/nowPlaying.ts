import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { findBlock } from '../mock';
import { navigate, onLeave } from '../router';
import { subscribe, ui } from '../state';
import { cover } from '../components/cover';

/**
 * Reproduciendo: the cover in big (or the video, for an MP4) with the same
 * picture blurred behind it. «Ver la playlist» opens the Reproductor on it.
 */
export function nowPlayingView(): HTMLElement {
  ui.lastPlayerPage = 'now-playing';
  const backdrop = h('div', { class: 'np-backdrop' });
  const art = h('div', { class: 'np-art' });
  const title = h('div', { class: 'np-title' });
  const meta = h('div', { class: 'subtitle' });
  const empty = h('p', { class: 'help', text: t('nowPlaying.empty') });
  const openPlaylist = h('button', { class: 'secondary-button', text: t('nowPlaying.openPlaylist'), onclick: () => {
    if (ui.nowPlaying) navigate('player', { b: ui.nowPlaying.blockKey });
  } });

  let shown = '';
  const refresh = () => {
    const np = ui.nowPlaying;
    empty.hidden = !!np;
    openPlaylist.hidden = !np;
    backdrop.hidden = !np;
    if (!np) {
      art.replaceChildren();
      title.textContent = '';
      meta.textContent = '';
      shown = '';
      return;
    }
    const key = `${np.blockKey}/${np.track.id}`;
    if (key === shown) return;
    shown = key;
    const found = findBlock(np.blockKey);
    const video = found?.block.video ?? false;
    art.replaceChildren(video
        ? h('div', { class: 'np-video' }, icon('play', 44, true))
        : cover(np.track.title, false, 320));
    // The same tile, blurred, behind everything (audio only: a video fills its own space).
    backdrop.replaceChildren(video ? '' : cover(np.track.title, false, 600));
    backdrop.hidden = video;
    title.textContent = np.track.title;
    meta.textContent = found ? `${found.playlist.title} · ${found.block.format}` : '';
  };
  onLeave(subscribe('player', refresh));
  refresh();

  return h('div', { class: 'screen np-screen' },
    backdrop,
    h('div', { class: 'np-tint' }),
    h('div', { class: 'np-content' },
      h('div', { class: 'title-row' },
        h('h1', { class: 'page-title', text: t('nowPlaying.title') }),
        h('div', { class: 'grow' }),
        openPlaylist),
      empty,
      art,
      h('div', { class: 'np-info' }, title, meta)));
}
