// One file in a list: its number (or the bars of what sounds), thumbnail, title
// and whatever goes on the right. The same row in «Ver archivos» and in a
// playlist of the Reproductor.

import { h } from '../dom';
import { icon } from '../icons';
import type { FormatBlock, Track } from '../api';
import { showPicture } from '../covers';
import { cover } from './cover';
import * as player from '../player';
import { isPlayable } from '../player';
import { ui } from '../state';

/** A file sounds if it is on disk, its format opens here and its folder can be reached. */
export function canPlayTrack(block: FormatBlock, track: Track): boolean {
  return track.status === 'ok' && isPlayable(block.format) && block.accessible !== false;
}

export function isCurrent(block: FormatBlock, track: Track): boolean {
  return ui.nowPlaying?.blockKey === block.key && ui.nowPlaying.track.id === track.id;
}

export function trackRow(block: FormatBlock, track: Track, right: Node, options: { landed?: boolean; thumbWidth: number; thumbHeight: number }): HTMLElement {
  const playable = canPlayTrack(block, track);
  const current = isCurrent(block, track);
  const number = current
    ? h('span', { class: `eq${ui.playing ? '' : ' paused'}` }, h('i'), h('i'), h('i'))
    : h('span', { class: 'num', text: String(track.number) });
  const picture = { kind: 'thumbnail', id: track.id } as const;
  const thumb = cover(track.title, block.video, options.thumbHeight, null);
  if (block.video) {
    thumb.style.width = `${options.thumbWidth}px`;
    showPicture(thumb, picture, options.thumbWidth / options.thumbHeight);
  } else {
    showPicture(thumb, picture);
  }
  const row = h('div', { class: `track track-row${current ? ' playing' : ''}${playable ? '' : ' off'}${options.landed ? ' landed' : ''}` },
    h('span', { class: 'n' }, number, h('span', { class: 'hover-play' }, icon(current && ui.playing ? 'pause' : 'play', 14, true))),
    thumb,
    h('span', { class: 't row-title ellipsis', text: track.title }),
    right);
  row.dataset.id = track.id;
  if (playable) {
    row.setAttribute('role', 'button');
    row.tabIndex = 0;
    const play = () => {
      if (isCurrent(block, track)) player.togglePlay();
      else player.play(block.key, track);
    };
    row.addEventListener('click', play);
    row.addEventListener('keydown', event => {
      if (event.key === 'Enter') { event.preventDefault(); play(); }
    });
  }
  return row;
}
