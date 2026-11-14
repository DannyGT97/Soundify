import { api, type PlaybackDevice } from '../api';
import { connect } from '../connect';
import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { findBlock } from '../library';
import { isPhone, onAndroid } from '../platform';
import { currentRoute, navigate, onLeave } from '../router';
import { currentIsVideo, hidePip, parkVideo, showPip, videoView } from '../player';
import { chooseAutoPip } from '../preferences';
import { subscribe, ui } from '../state';
import { cover } from '../components/cover';
import { buildTransport, buildVolume, describe } from '../components/transport';
import { pipButton } from '../components/pip';
import { openDevicesSwitcher } from '../overlays/devicesSwitcher';

/** How long the controls stay over a full-screen video after the mouse stops. */
const CONTROLS_IDLE_MS = 2500;
/** How often the playback device is asked again while this screen is open. */
const DEVICE_POLL_MS = 3000;

/**
 * Reproduciendo: the cover in big (or the video, for an MP4) with the same
 * picture blurred behind it. The arrow, at the top right, goes to the playlist that is playing.
 * The controls show here when the bottom bar does not (only the side menu's
 * mini player, which cannot seek), and a video can go full screen.
 */
export function nowPlayingView(): HTMLElement {
  ui.lastPlayerPage = 'now-playing';
  // The panel that opened by itself on the way out goes back in on the way in.
  if (ui.pip && ui.pipAuto) hidePip();
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
  // Back to the playlist that is playing, in the Reproductor (its list when nothing is); while another device is
  // controlled, to its playlist (a remote playlist: the songs are there, and one can be picked from here).
  const back = h('button', { class: 'icon-button np-back', onclick: () => {
    if (ui.remote) navigate('remote-playlist');
    else if (ui.nowPlaying) navigate('player', { b: ui.nowPlaying.blockKey });
    else navigate('player');
  } }, icon('back', 22));
  const paintBack = () => {
    const label = t(ui.remote ? 'nowPlaying.backRemote' : 'nowPlaying.back');
    back.title = label;
    back.setAttribute('aria-label', label);
    controls.classList.toggle('remote', !!ui.remote);
  };
  // Where the sound goes: this device, or the headphones / Bluetooth device that is connected.
  const deviceIcon = h('span', { class: 'np-device-icon' });
  const deviceName = h('span', { class: 'np-device-name ellipsis' });
  // It opens the list of devices (Soundify Connect): to control another one, or to bring the music back.
  const device = h('button', { class: 'np-device', title: t('nowPlaying.device'), onclick: openDevicesSwitcher }, deviceIcon, deviceName);
  const paintDevice = (found: PlaybackDevice) => {
    // Controlling another Soundify: that is where it sounds.
    if (ui.remote) {
      deviceIcon.replaceChildren(icon('monitor', 16));
      deviceName.textContent = ui.remote.name;
      device.classList.add('external');
      return;
    }
    deviceIcon.replaceChildren(icon(found.kind === 'bluetooth' ? 'bluetooth' : found.kind === 'headphones' ? 'headphones' : onAndroid ? 'device' : 'monitor', 16));
    deviceName.textContent = found.name || t('nowPlaying.device.this');
    device.classList.toggle('external', found.kind !== 'this');
  };
  const fullscreenButton = h('button', { class: 'player-control np-fullscreen-button', onclick: () => setFullscreen(!fullscreen) });
  const controls = h('div', { class: 'np-controls' }, buildTransport(), buildVolume(), fullscreenButton);

  // Whether a video floats by itself when this screen is left (a preference, also in Ajustes).
  const autoPip = h('button', { class: 'pip-auto', attrs: { type: 'button' }, onclick: () => chooseAutoPip(!ui.autoPip) },
    icon('pip', 14), h('span', { text: t('pip.auto') }));
  const paintAuto = () => {
    autoPip.hidden = !currentIsVideo();
    autoPip.classList.toggle('on', ui.autoPip);
    autoPip.setAttribute('aria-pressed', String(ui.autoPip));
    autoPip.title = t(ui.autoPip ? 'pip.auto.on' : 'pip.auto.off');
  };

  const info = h('div', { class: 'np-info' }, title, meta, device, autoPip);
  // The window's title: the playlist that sounds (and, when it is on another device, that it is a remote playlist).
  const headEyebrow = h('div', { class: 'np-head-eyebrow' });
  const headTitle = h('div', { class: 'np-head-name ellipsis' });
  const head = h('div', { class: 'np-head-title grow min0' }, headEyebrow, headTitle);
  const paintHead = () => {
    const np = ui.nowPlaying;
    const name = !np ? '' : ui.remote ? (connect.queue?.playlist ?? '') : findBlock(np.blockKey)?.playlist.title ?? '';
    headEyebrow.textContent = np && ui.remote ? t('remotePlaylist.eyebrow') : '';
    headEyebrow.hidden = !(np && ui.remote);
    headTitle.textContent = name;
    headTitle.title = name;
  };

  const screen = h('div', { class: 'screen np-screen' },
    backdrop,
    h('div', { class: 'np-tint' }),
    h('div', { class: 'np-content' },
      h('div', { class: 'np-head' }, head, back),
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
    controls.hidden = !ui.nowPlaying || (ui.playerPlacement !== 'side' && !fullscreen && !isPhone());
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
  screen.addEventListener('pointerdown', wake);
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
          icon('pip', 28),
          h('span', { class: 'pip-here-text', text: t('pip.here') }),
          h('span', { class: 'pip-here-hint', text: t('pip.dockHint') }),
          h('button', { class: 'secondary-button', text: t('pip.bringBack'), onclick: hidePip }))
        : video
        ? view ?? h('div', { class: 'np-video' }, icon('play', 44, true))
        : cover(np.track.title, false, 320, { kind: 'thumbnail', id: np.track.id }));
    // The same tile, blurred, behind everything (audio only: a video fills its own space).
    backdrop.replaceChildren(video ? '' : cover(np.track.title, false, 600, { kind: 'thumbnail', id: np.track.id }));
    backdrop.hidden = video;
    title.textContent = np.track.title;
    meta.textContent = found ? found.block.format : '';
  };
  onLeave(subscribe('player', refresh));
  onLeave(subscribe('player', paintHead));
  onLeave(subscribe('connect', paintHead));
  onLeave(subscribe('player', paintAuto));
  onLeave(subscribe('player-placement', refreshControls));
  // Back to its hidden holder in the same task: out of the page, it would pause.
  onLeave(parkVideo);
  // After that, so the element is already parked when the panel takes it. Only
  // for another screen: being drawn again (a language change) is not leaving.
  onLeave(() => {
    if (currentRoute().route !== 'now-playing' && ui.autoPip && !ui.pip) showPip(true);
  });
  // Which device it sounds on can change under the screen (headphones connected): asked every few seconds.
  paintDevice({ kind: 'this', name: null });
  const pullDevice = () => void api.playbackDevice().then(paintDevice).catch(() => {});
  pullDevice();
  onLeave(subscribe('connect', pullDevice));
  onLeave(subscribe('connect', paintBack));
  paintBack();
  paintHead();
  const deviceTimer = window.setInterval(() => document.visibilityState === 'visible' && pullDevice(), DEVICE_POLL_MS);
  onLeave(() => window.clearInterval(deviceTimer));
  refresh();
  paintAuto();
  return screen;
}
