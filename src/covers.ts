// The real pictures on the covers (the Java `CoverArt`): a playlist's own,
// picked by the user, or else its first video's thumbnail; the bar and Now
// Playing take the track's own video thumbnail. They sit ON TOP of the
// gradient tile of `components/cover.ts`, which stays underneath as the
// fallback while a picture loads or when there is none.

import { api, type Playlist } from './api';

/** What a cover shows on top of its tile. */
export type Picture =
  | { kind: 'file'; path: string }
  | { kind: 'thumbnail'; id: string };

/** The part of a picture to show: {x, y, width, height}, in the picture's own pixels. */
export interface CropRect { x: number; y: number; width: number; height: number }

const YOUTUBE_FRAME = 4 / 3;
const YOUTUBE_PICTURE = 16 / 9;

// === PURE LOGIC ===

/**
 * The part to cut out of a `width`×`height` picture to fill a cover of shape
 * `ratio` (width / height; 1 = square), centred. For a YouTube thumbnail
 * (`youtubeFrame`) of 4:3 shape it first leaves out the black bands above and
 * below the 16:9 picture — cutting the centre of the whole frame kept a strip
 * of black at the top and at the bottom of every cover.
 */
export function crop(width: number, height: number, youtubeFrame: boolean, ratio: number): CropRect {
  let usableHeight = height;
  let top = 0;
  if (youtubeFrame && Math.abs(width / height - YOUTUBE_FRAME) < 0.02) {
    usableHeight = width / YOUTUBE_PICTURE;
    top = (height - usableHeight) / 2;
  }
  let cutWidth = width;
  let cutHeight = usableHeight;
  if (width / usableHeight > ratio) cutWidth = usableHeight * ratio;
  else cutHeight = width / ratio;
  return { x: (width - cutWidth) / 2, y: top + (usableHeight - cutHeight) / 2, width: cutWidth, height: cutHeight };
}

/** The picture a playlist's cover shows: the picked one, otherwise its first format's first video. */
export function playlistPicture(playlist: Playlist): Picture | null {
  if (playlist.cover) return { kind: 'file', path: playlist.cover };
  const id = playlist.formats.map(b => b.firstVideoId).find(Boolean);
  return id ? { kind: 'thumbnail', id } : null;
}

/**
 * How the `<img>` sits inside its tile so that only `rect` shows, in
 * percentages of the tile: they need no layout, so the cover can be built
 * before it is on screen.
 */
export function placement(naturalWidth: number, naturalHeight: number, rect: CropRect): { left: string; top: string; width: string; height: string } {
  const percent = (n: number) => `${Math.round(n * 10000) / 100}%`;
  return {
    left: percent(-rect.x / rect.width),
    top: percent(-rect.y / rect.height),
    width: percent(naturalWidth / rect.width),
    height: percent(naturalHeight / rect.height),
  };
}

// === ON SCREEN ===

/**
 * Lays `picture` over `tile` once it has loaded (never a broken image: until
 * then, or if it fails, the tile stays as it is). A tile reused for another
 * picture meanwhile ignores the late one.
 */
export function showPicture(tile: HTMLElement, picture: Picture | null, ratio = 1): void {
  const key = picture ? `${picture.kind}:${picture.kind === 'file' ? picture.path : picture.id}` : '';
  tile.dataset.picture = key;
  tile.querySelector(':scope > img.cover-img')?.remove();
  if (!picture) return;
  void sourceOf(picture).then(source => {
    if (!source || tile.dataset.picture !== key) return;
    const img = new Image();
    img.className = 'cover-img';
    img.alt = '';
    img.onload = () => {
      if (tile.dataset.picture !== key) return;
      const rect = crop(img.naturalWidth, img.naturalHeight, picture.kind === 'thumbnail', ratio);
      Object.assign(img.style, placement(img.naturalWidth, img.naturalHeight, rect));
      tile.querySelector(':scope > img.cover-img')?.remove();
      tile.append(img);
    };
    img.src = source;
  });
}

// === PRIVATE HELPERS ===

const waiting = new Map<string, ((path: string) => void)[]>();
let listening = false;

async function sourceOf(picture: Picture): Promise<string | null> {
  if (picture.kind === 'file') return api.fileUrl(picture.path);
  const path = await thumbnailPath(picture.id);
  return path ? api.fileUrl(path) : null;
}

/**
 * The thumbnail's file. When it is not on disk yet the back fetches it and
 * says so with `thumbnail-ready`; the wait is set up BEFORE asking, so an
 * answer that lands in between is not missed.
 */
async function thumbnailPath(id: string): Promise<string | null> {
  if (!listening) {
    listening = true;
    api.onThumbnailReady(ready => {
      const listeners = waiting.get(ready);
      waiting.delete(ready);
      if (listeners) void api.thumbnail(ready).then(file => file && listeners.forEach(resolve => resolve(file)));
    });
  }
  let resolveLater: (path: string) => void = () => {};
  // A failed one never arrives: its cover simply stays the tile.
  const arrived = new Promise<string>(resolve => { resolveLater = resolve; });
  waiting.set(id, [...(waiting.get(id) ?? []), resolveLater]);
  const path = await api.thumbnail(id).catch(() => null);
  if (!path) return arrived;
  const rest = (waiting.get(id) ?? []).filter(listener => listener !== resolveLater);
  if (rest.length) waiting.set(id, rest);
  else waiting.delete(id);
  return path;
}
