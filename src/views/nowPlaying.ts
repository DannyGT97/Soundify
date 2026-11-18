import { api, type PlaybackDevice } from '../api';
import { connect } from '../connect';
import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { findBlock } from '../library';
import { isPhone, onAndroid } from '../platform';
import { currentRoute, navigate, onLeave } from '../router';
import { castBlockInfo, castHere, currentIsVideo, hidePip, parkVideo, showPip, videoView } from '../player';
import { chooseAutoPip } from '../preferences';
import { subscribe, ui } from '../state';
import { cover } from '../components/cover';
import { playlistPicture, queuePicture } from '../covers';
import { buildTransport, buildVolume, describe } from '../components/transport';
import { pipButton } from '../components/pip';
import { activityButtons } from '../components/activityButtons';
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
  const goToPlaylist = () => {
    // What another device passes here has no playlist of this one: its own view, with the way back to this device's.
    if (castHere()) navigate('cast-queue');
    else if (ui.remote) navigate('remote-playlist');
    else if (ui.nowPlaying) navigate('player', { b: ui.nowPlaying.blockKey });
    else navigate('player');
  };
  const paintBack = () => {
    controls.classList.toggle('remote', !!ui.remote);
  };
  // Where the sound goes: this device, or the headphones / Bluetooth device that is connected.
  const deviceIcon = h('span', { class: 'np-device-icon' });
  const deviceName = h('span', { class: 'np-device-name ellipsis' });
  // It opens the list of devices (Soundify Connect): to control another one, or to bring the music back.
  const device = h('button', { class: 'np-device', title: t('nowPlaying.device'), onclick: openDevicesSwitcher }, deviceIcon, deviceName);
  const paintDevice = (found: PlaybackDevice) => {
    // This device's music on another Soundify («Escuchar en»), or another one controlled from here: that is where it sounds.
    device.classList.toggle('cast', !!ui.castTo);
    device.classList.toggle('control', !ui.castTo && !!ui.remote);
    if (ui.castTo) {
      deviceIcon.replaceChildren(icon('cast', 16));
      deviceName.textContent = ui.castTo.name;
      device.classList.add('external');
      return;
    }
    if (ui.remote) {
      deviceIcon.replaceChildren(icon('control', 16));
      deviceName.textContent = t('connect.controlling', ui.remote.name);
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
  // Pressing it goes to the playlist: the arrow at its left says it is also the way back, so there is no other arrow.
  const backArrow = h('span', { class: 'np-back' }, icon('back', 20));
  const headCover = h('span', { class: 'np-head-cover' });
  const head = h('button', { class: 'np-head-title np-head-link min0', attrs: { type: 'button' }, onclick: goToPlaylist },
    backArrow, headCover, h('span', { class: 'np-head-text min0' }, headEyebrow, headTitle));
  const paintHead = () => {
    const np = ui.nowPlaying;
    const passed = np ? castHere() : null;
    const found = np && !ui.remote && !passed ? findBlock(np.blockKey) : null;
    const name = !np ? '' : passed ? passed.playlist : ui.remote ? (connect.queue?.playlist ?? '') : found?.playlist.title ?? '';
    // Another device's music played here: whose it is (there is no playlist of this device to go back to).
    headEyebrow.textContent = passed ? t('connect.castFrom', passed.from) : np && ui.remote ? t('remotePlaylist.eyebrow') : '';
    headEyebrow.hidden = !(np && (ui.remote || passed));
    headTitle.textContent = name;
    head.title = passed ? t('nowPlaying.backCast') : name ? t(ui.remote ? 'nowPlaying.backRemote' : 'nowPlaying.back') : '';
    head.disabled = !name;
    if (!name) headCover.replaceChildren();
    else if (passed) headCover.replaceChildren(cover(name, false, 36));
    else if (ui.remote) headCover.replaceChildren(cover(name, false, 36, queuePicture(connect.queue)));
    else if (found) headCover.replaceChildren(cover(name, found.block.video, 36, playlistPicture(found.playlist)));
  };

  const screen = h('div', { class: 'screen np-screen' },
    backdrop,
    h('div', { class: 'np-tint' }),
    h('div', { class: 'np-content' },
      h('div', { class: 'np-head' }, head, activityButtons()),
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
    meta.textContent = found ? found.block.format : castBlockInfo()?.format ?? '';
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
