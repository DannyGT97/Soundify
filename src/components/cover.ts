import { h } from '../dom';
import { showPicture, type Picture } from '../covers';

/**
 * The tile under every cover: the title's initial on a gradient that tells the
 * kind apart (violet video, green audio). The real picture, when there is one
 * (chosen cover, video thumbnail), is laid on top once it loads (`covers.ts`),
 * as in the Java app: until then, or without one, the tile is what shows.
 */
export function cover(title: string, video: boolean, size: number, picture: Picture | null = null): HTMLElement {
  const initial = (title.trim()[0] ?? 'S').toUpperCase();
  const tile = h('div', { class: `cover ${video ? 'cover-video' : 'cover-audio'}` },
    h('span', { text: initial }));
  tile.style.width = `${size}px`;
  tile.style.height = `${size}px`;
  tile.style.fontSize = `${Math.round(size * 0.42)}px`;
  showPicture(tile, picture);
  return tile;
}
