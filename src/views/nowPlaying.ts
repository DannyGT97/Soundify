import { api } from '../api';
import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { findBlock } from '../library';
import { navigate, onLeave } from '../router';
import { hidePip, parkVideo, videoView } from '../player';
import { subscribe, ui } from '../state';
import { cover } from '../components/cover';
import { buildTransport, buildVolume, describe } from '../components/transport';
import { pipButton } from '../components/pip';

/** How long the controls stay over a full-screen video after the mouse stops. */
const CONTROLS_IDLE_MS = 2500;

/**
 * Reproduciendo: the cover in big (or the video, for an MP4) with the same
 * picture blurred behind it. «Ver la playlist» opens the Reproductor on it.
 * The controls show here when the bottom bar does not (only the side menu's
 * mini player, which cannot seek), and a video can go full screen.
 */
export function nowPlayingView(): HTMLElement {
  ui.lastPlayerPage = 'now-playing';
  const backdrop = h('div', { class: 'np-backdrop' });
  // What is shown, and over it the way into the floating panel.
  const stage = h('div', { class: 'np-stage' });
  const art = h('div', { class: 'np-art' }, stage, pipButton());
  const title = h('div', { class: 'np-title' });
  const meta = h('div', { class: 'subtitle' });
  const empty = h('div', { class: 'now-empty' },
    h('div', { class: 'empty' },
      h('div', { class: 'empty-mark' }, icon('music', 32)),
      h('h2', { text: t('nowPlaying.empty.title') }),
      h('p', { text: t('nowPlaying.empty.body') }),
      h('div', { class: 'row gap-8' },
        h('button', { class: 'secondary-button', onclick: () => navigate('player') }, icon('list', 16), h('span', { text: t('nowPlaying.goToPlayer') })))));
  const openPlaylist = h('button', { class: 'secondary-button', text: t('nowPlaying.openPlaylist'), onclick: () => {
    if (ui.nowPlaying) navigate('player', { b: ui.nowPlaying.blockKey });
  } });
  const fullscreenButton = h('button', { class: 'player-control np-fullscreen-button', onclick: () => setFullscreen(!fullscreen) });
  const controls = h('div', { class: 'np-controls' }, buildTransport(), buildVolume(), fullscreenButton);

  const info = h('div', { class: 'np-info' }, title, meta);

  const screen = h('div', { class: 'screen np-screen' },
    backdrop,
    h('div', { class: 'np-tint' }),
    h('div', { class: 'np-content' },
      h('div', { class: 'title-row np-head' },
        h('h1', { class: 'page-title', text: t('nowPlaying.title') }),
        h('div', { class: 'grow' }),
        openPlaylist),
      empty,
      art,
      info,
      controls));

  // === FULL SCREEN ===
  // The window goes full screen (the system's: inside the webview a page's own
  // full screen only fills the window) and this screen covers it. The controls
  // float over the video and fade while the mouse rests.
  let fullscreen = false;
  let idleTimer = 0;
  const isVideo = () => !!ui.nowPlaying && (findBlock(ui.nowPlaying.blockKey)?.block.video ?? false);
  const refreshControls = () => {
    controls.hidden = !ui.nowPlaying || (ui.playerPlacement !== 'side' && !fullscreen);
    fullscreenButton.hidden = !isVideo();
    fullscreenButton.replaceChildren(icon(fullscreen ? 'fullscreen-exit' : 'fullscreen'));
    describe(fullscreenButton, fullscreen ? 'nowPlaying.exitFullscreen' : 'nowPlaying.fullscreen');
  };
  const wake = () => {
    screen.classList.remove('np-idle');
    window.clearTimeout(idleTimer);
    if (fullscreen) idleTimer = window.setTimeout(() => screen.classList.add('np-idle'), CONTROLS_IDLE_MS);
  };
  const setFullscreen = (on: boolean) => {
    if (on === fullscreen) return;
    fullscreen = on;
    // Full screen needs the picture here.
    if (on) hidePip();
    screen.classList.toggle('np-fullscreen', on);
    api.setFullscreen(on);
    refreshControls();
    wake();
  };
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && fullscreen) setFullscreen(false);
  };
  screen.addEventListener('mousemove', wake);
  art.addEventListener('dblclick', () => { if (isVideo()) setFullscreen(!fullscreen); });
  window.addEventListener('keydown', onKey);
  onLeave(() => {
    window.removeEventListener('keydown', onKey);
    window.clearTimeout(idleTimer);
    setFullscreen(false);
  });

  let shown = '';
  const refresh = () => {
    const np = ui.nowPlaying;
    empty.hidden = !!np;
    art.hidden = !np;
    info.hidden = !np;
    openPlaylist.hidden = !np;
    backdrop.hidden = !np;
    if (!np) {
      setFullscreen(false);
      refreshControls();
      stage.replaceChildren();
      title.textContent = '';
      meta.textContent = '';
      shown = '';
      return;
    }
    const key = `${np.blockKey}/${np.track.id}/${ui.pip}`;
    if (key === shown) return;
    shown = key;
    const found = findBlock(np.blockKey);
    const video = found?.block.video ?? false;
    // An audio file has nothing to fill the screen with.
    if (!video) setFullscreen(false);
    refreshControls();
    // The engine's own element, moved in (never a copy: the picture IS the
    // playback); where the engine draws nothing (the mock), a placeholder.
    const view = video ? videoView() : null;
    if (!video) parkVideo();
    stage.replaceChildren(video && ui.pip
        ? h('div', { class: 'np-video np-pip-here' },
          h('span', { text: t('pip.here') }),
          h('button', { class: 'secondary-button', text: t('pip.bringBack'), onclick: hidePip }))
        : video
        ? view ?? h('div', { class: 'np-video' }, icon('play', 44, true))
        : cover(np.track.title, false, 320, { kind: 'thumbnail', id: np.track.id }));
    // The same tile, blurred, behind everything (audio only: a video fills its own space).
    backdrop.replaceChildren(video ? '' : cover(np.track.title, false, 600, { kind: 'thumbnail', id: np.track.id }));
    backdrop.hidden = video;
    title.textContent = np.track.title;
    meta.textContent = found ? `${found.playlist.title} · ${found.block.format}` : '';
  };
  onLeave(subscribe('player', refresh));
  onLeave(subscribe('player-placement', refreshControls));
  // Back to its hidden holder in the same task: out of the page, it would pause.
  onLeave(parkVideo);
  refresh();
  return screen;
}
