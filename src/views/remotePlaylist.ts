// The playlist of the device being controlled (Soundify Connect): what it is playing, listed here so a song can be
// picked from this device. Technically a «remote playlist»: the files are on that device, and it says which one.

import type { ConnectQueueItem } from '../api';
import { cover } from '../components/cover';
import { connect, playRemoteTrack } from '../connect';
import { hueOf, queuePicture, showPicture } from '../covers';
import { h } from '../dom';
import { t } from '../i18n';
import { icon } from '../icons';
import { navigate, onLeave } from '../router';
import { subscribe, ui } from '../state';

function numberCell(current: boolean, number: number): HTMLElement {
  return current
    ? h('span', { class: `eq${ui.playing ? '' : ' paused'}` }, h('i'), h('i'), h('i'))
    : h('span', { class: 'num', text: String(number) });
}

function stateOf(item: ConnectQueueItem): 'playing' | 'paused' | 'idle' {
  if (ui.nowPlaying?.track.id !== item.videoId) return 'idle';
  return ui.playing ? 'playing' : 'paused';
}

function row(item: ConnectQueueItem, number: number): HTMLElement {
  const state = stateOf(item);
  const current = state !== 'idle';
  // The thumbnail of the video, by its id (the same one the local lists use: it is fetched if this device does not have it).
  const thumb = cover(item.title, false, 40);
  showPicture(thumb, { kind: 'thumbnail', id: item.videoId });
  const element = h('div', { class: `track track-row${current ? ' playing' : ''}`, attrs: { role: 'button', tabindex: '0' } },
    h('span', { class: 'n' }, numberCell(current, number), h('span', { class: 'hover-play' }, icon(state === 'playing' ? 'pause' : 'play', 14, true))),
    thumb,
    h('span', { class: 't row-title ellipsis', text: item.title }),
    h('span', { class: 'd' }));
  element.dataset.id = item.videoId;
  element.dataset.number = String(number);
  element.dataset.state = state;
  element.addEventListener('click', () => playRemoteTrack(item.videoId));
  element.addEventListener('keydown', event => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    playRemoteTrack(item.videoId);
  });
  return element;
}

export function remotePlaylistView(): HTMLElement {
  const device = ui.remote;
  // Nothing is being controlled: there is no remote playlist to show.
  if (!device) {
    queueMicrotask(() => navigate('player'));
    return h('div', { class: 'screen' });
  }
  const back = () => navigate('now-playing');
  const backButton = (extra = '') => h('button', { class: `icon-button${extra}`, title: t('player.back'), onclick: back }, icon('back', 18));
  const heading = h('h1');
  const sub = h('div', { class: 'pl-sub' });
  const tracks = h('div', { class: 'tracks' });
  const none = h('p', { class: 'none', text: t('remotePlaylist.empty') });
  const miniTitle = h('b', { class: 'ellipsis grow' });
  const heroCover = cover(device.name, false, 168);
  let shownFor: object | null | undefined;

  const fill = () => {
    const queue = connect.queue;
    shownFor = queue;
    const title = queue?.playlist || t('remotePlaylist.eyebrow');
    heading.textContent = title;
    heading.title = title;
    miniTitle.textContent = title;
    detail.style.setProperty('--h', String(hueOf(title)));
    sub.textContent = [t('remotePlaylist.on', device.name), queue ? t('remotePlaylist.files', queue.items.length) : ''].filter(Boolean).join(' · ');
    showPicture(heroCover, queuePicture(queue));
    tracks.replaceChildren(...(queue?.items ?? []).map((item, index) => row(item, index + 1)));
    none.hidden = !!queue?.items.length;
    tracks.querySelector('.playing')?.scrollIntoView({ block: 'center' });
  };

  /** What sounds changes all the time: only the rows whose state changed are touched (the list can have hundreds). */
  const paint = () => {
    for (const element of tracks.querySelectorAll<HTMLElement>(':scope > .track-row')) {
      const item = { videoId: element.dataset.id ?? '', title: '' };
      const state = stateOf(item);
      if (element.dataset.state === state) continue;
      element.dataset.state = state;
      element.classList.toggle('playing', state !== 'idle');
      const n = element.querySelector<HTMLElement>(':scope > .n');
      n?.querySelector(':scope > .eq, :scope > .num')?.replaceWith(numberCell(state !== 'idle', Number(element.dataset.number)));
      n?.querySelector(':scope > .hover-play')?.replaceChildren(icon(state === 'playing' ? 'pause' : 'play', 14, true));
    }
  };

  const hero = h('div', { class: 'ambient' },
    backButton(' back'),
    h('div', { class: 'hero' },
      h('span', { class: 'hero-cover' }, heroCover),
      h('div', { class: 'grow min0' },
        h('div', { class: 'eyebrow', text: t('remotePlaylist.eyebrow') }),
        heading,
        sub)));
  const scroller = h('div', { class: 'scroller' }, hero, tracks, none);
  const detail = h('div', { class: 'detail' },
    h('div', { class: 'mini' }, icon('monitor', 18), miniTitle, h('span', { class: 'fmt', text: device.name }), backButton()),
    scroller);
  scroller.addEventListener('scroll', () => detail.classList.toggle('stuck', scroller.scrollTop > hero.offsetHeight - 56));

  onLeave(subscribe('connect', () => {
    // The device is not being controlled any more: back to the player.
    if (!ui.remote) return navigate('player');
    if (connect.queue !== shownFor) fill();
  }));
  onLeave(subscribe('player', paint));
  const onKey = (event: KeyboardEvent) => {
    if (event.key !== 'Escape' || document.querySelector('#overlay-root')?.childElementCount) return;
    back();
  };
  window.addEventListener('keydown', onKey);
  onLeave(() => window.removeEventListener('keydown', onKey));

  fill();
  return h('div', { class: 'screen detail-screen' }, detail);
}
