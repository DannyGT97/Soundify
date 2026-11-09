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

function numberCell(track: Track, current: boolean): HTMLElement {
  return current
    ? h('span', { class: `eq${ui.playing ? '' : ' paused'}` }, h('i'), h('i'), h('i'))
    : h('span', { class: 'num', text: String(track.number) });
}

function playIcon(current: boolean): SVGElement {
  return icon(current && ui.playing ? 'pause' : 'play', 14, true);
}

/**
 * Brings the rows already on screen up to date with what is playing, in place:
 * the bars, the play/pause icon and the highlight of the rows whose state
 * changed. Rebuilding the list for that threw away every thumbnail `<img>`,
 * and they all blinked until the `asset` protocol served them again.
 */
export function patchPlaying(list: HTMLElement, block: FormatBlock): void {
  const byId = new Map(block.tracks.map(track => [track.id, track]));
  for (const row of list.querySelectorAll<HTMLElement>(':scope > .track-row')) {
    const track = byId.get(row.dataset.id ?? '');
    if (!track) continue;
    const current = isCurrent(block, track);
    const state = current ? (ui.playing ? 'playing' : 'paused') : 'idle';
    if (row.dataset.state === state) continue;
    row.dataset.state = state;
    row.classList.toggle('playing', current);
    const n = row.querySelector<HTMLElement>(':scope > .n');
    if (!n) continue;
    n.querySelector(':scope > .eq, :scope > .num')?.replaceWith(numberCell(track, current));
    n.querySelector(':scope > .hover-play')?.replaceChildren(playIcon(current));
  }
}

export function trackRow(block: FormatBlock, track: Track, right: Node, options: { landed?: boolean; thumbWidth: number; thumbHeight: number }): HTMLElement {
  const playable = canPlayTrack(block, track);
  const current = isCurrent(block, track);
  const number = numberCell(track, current);
  const picture = { kind: 'thumbnail', id: track.id } as const;
  const thumb = cover(track.title, block.video, options.thumbHeight, null);
  if (block.video) {
    thumb.style.width = `${options.thumbWidth}px`;
    showPicture(thumb, picture, options.thumbWidth / options.thumbHeight);
  } else {
    showPicture(thumb, picture);
  }
  const row = h('div', { class: `track track-row${current ? ' playing' : ''}${playable ? '' : ' off'}${options.landed ? ' landed' : ''}` },
    h('span', { class: 'n' }, number, h('span', { class: 'hover-play' }, playIcon(current))),
    thumb,
    h('span', { class: 't row-title ellipsis', text: track.title }),
    right);
  row.dataset.id = track.id;
  row.dataset.state = current ? (ui.playing ? 'playing' : 'paused') : 'idle';
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
