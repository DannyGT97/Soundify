import { h } from '../dom';

/**
 * The tile under every cover: the title's initial on a gradient that tells the
 * kind apart (violet video, green audio). Real pictures (chosen cover, video
 * thumbnail, embedded artwork) will sit on top of it, as in the Java app.
 */
export function cover(title: string, video: boolean, size: number): HTMLElement {
  const initial = (title.trim()[0] ?? 'S').toUpperCase();
  const tile = h('div', { class: `cover ${video ? 'cover-video' : 'cover-audio'}` },
    h('span', { text: initial }));
  tile.style.width = `${size}px`;
  tile.style.height = `${size}px`;
  tile.style.fontSize = `${Math.round(size * 0.42)}px`;
  return tile;
}
