// What another device passes to this one («Escuchar en» from the sender): its playlist as it arrives, to see what comes
// and pick a song of it. While it lasts this device is only the speaker: the sender is in charge, and this device's own
// playlists wait (Reproductor shows them locked) until «Escuchar en este dispositivo» ends it.
// Not the remote playlist of `remotePlaylist.ts` (that one is the playlist of a device that is CONTROLLED from here).

import { cover } from '../components/cover';
import { stopListening } from '../cast';
import { showPicture } from '../covers';
import { h } from '../dom';
import { t } from '../i18n';
import { icon } from '../icons';
import * as player from '../player';
import { navigate, onLeave } from '../router';
import { subscribe, ui } from '../state';

function numberCell(current: boolean, number: number): HTMLElement {
  return current
    ? h('span', { class: `eq${ui.playing ? '' : ' paused'}` }, h('i'), h('i'), h('i'))
    : h('span', { class: 'num', text: String(number) });
}

export function castQueueView(): HTMLElement {
  const now = player.castNow();
  // Nothing is passed here (it ended while this was open, or somebody typed the address): back to the player.
  if (!now) {
    queueMicrotask(() => navigate('player'));
    return h('div', { class: 'screen' });
  }
  const { from, playlist, items } = now;
  const heroCover = cover(playlist, false, 168);
  const first = items[0];
  if (first) showPicture(heroCover, { kind: 'thumbnail', id: first.id });

  const play = h('button', { class: 'round-play big', onclick: () => player.togglePlay() });
  const leave = h('button', { class: 'primary-button cast-leave', onclick: stopListening }, icon('device', 16), h('span', { text: t('castQueue.listenHere') }));
  const tracks = h('div', { class: 'tracks cast-list' });

  const rows = items.map((item, index) => {
    const thumb = cover(item.title, false, 40);
    showPicture(thumb, { kind: 'thumbnail', id: item.id });
    const row = h('div', { class: 'track track-row', attrs: { role: 'button', tabindex: '0' } },
      h('span', { class: 'n' }, numberCell(false, index + 1), h('span', { class: 'hover-play' }, icon('play', 14, true))),
      thumb,
      h('span', { class: 't row-title ellipsis', text: item.title }),
      h('span', { class: 'd' }));
    row.dataset.id = item.id;
    row.dataset.number = String(index + 1);
    const pick = () => {
      if (ui.nowPlaying?.track.id === item.id) player.togglePlay();
      else player.playTrackById(item.id);
    };
    row.addEventListener('click', pick);
    row.addEventListener('keydown', event => {
      if (event.key !== 'Enter') return;
      event.preventDefault();
      pick();
    });
    return row;
  });
  tracks.replaceChildren(...rows);

  /** What sounds changes all the time: only the rows whose state changed are touched (the queue can have hundreds). */
  const paint = () => {
    play.replaceChildren(icon(ui.playing ? 'pause' : 'play', 24, true));
    play.title = t(ui.playing ? 'player.pause' : 'player.play');
    play.setAttribute('aria-label', play.title);
    for (const row of rows) {
      const current = ui.nowPlaying?.track.id === row.dataset.id;
      const state = current ? (ui.playing ? 'playing' : 'paused') : 'idle';
      if (row.dataset.state === state) continue;
      row.dataset.state = state;
      row.classList.toggle('playing', current);
      const n = row.querySelector<HTMLElement>(':scope > .n');
      n?.querySelector(':scope > .eq, :scope > .num')?.replaceWith(numberCell(current, Number(row.dataset.number)));
      n?.querySelector(':scope > .hover-play')?.replaceChildren(icon(state === 'playing' ? 'pause' : 'play', 14, true));
    }
  };

  const back = () => navigate('now-playing');
  const backButton = (extra = '') => h('button', { class: `icon-button${extra}`, title: t('player.back'), onclick: back }, icon('back', 18));
  const hero = h('div', { class: 'ambient' },
    backButton(' back'),
    h('div', { class: 'hero' },
      h('span', { class: 'hero-cover cast-edge' }, heroCover),
      h('div', { class: 'grow min0' },
        h('div', { class: 'eyebrow cast-eyebrow' }, icon('cast', 15), h('span', { text: t('castQueue.eyebrow', from) })),
        h('h1', { text: playlist, attrs: { title: playlist } }),
        h('div', { class: 'pl-sub' }, t('castQueue.sub', items.length), ' ', h('span', { class: 'where remote' }, icon('cast', 14), h('span', { text: from }))))),
    h('div', { class: 'actions' }, play, leave));
  const callout = h('div', { class: 'cast-callout' }, icon('info', 18),
    h('div', {}, h('b', { text: t('castQueue.callout.title', from) }), h('span', { text: t('castQueue.callout.body') })));
  const scroller = h('div', { class: 'scroller' }, hero, callout, tracks);
  const detail = h('div', { class: 'detail' }, scroller);

  paint();
  onLeave(subscribe('player', () => {
    // The pass ended while this is open (the sender took it back): nothing to show.
    if (!player.castHere()) {
      navigate('player');
      return;
    }
    paint();
  }));
  return h('div', { class: 'screen detail-screen' }, detail);
}
