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
  const place = (source: string, width: number, height: number) => {
    const img = new Image();
    img.className = 'cover-img';
    img.alt = '';
    Object.assign(img.style, placement(width, height, crop(width, height, picture.kind === 'thumbnail', ratio)));
    img.src = source;
    return img;
  };
  // A thumbnail seen before is laid on in the same tick, so a list that is
  // rebuilt does not show the bare tile first. Not a picked cover: it is always
  // called `cover.jpg`, and a new one would show the old one.
  const seen = picture.kind === 'thumbnail' ? loaded.get(key) : undefined;
  if (seen) {
    tile.append(place(seen.source, seen.width, seen.height));
    return;
  }
  void sourceOf(picture).then(source => {
    if (!source || tile.dataset.picture !== key) return;
    const probe = new Image();
    probe.onload = () => {
      if (tile.dataset.picture !== key) return;
      if (picture.kind === 'thumbnail') loaded.set(key, { source, width: probe.naturalWidth, height: probe.naturalHeight });
      tile.querySelector(':scope > img.cover-img')?.remove();
      tile.append(place(source, probe.naturalWidth, probe.naturalHeight));
    };
    probe.src = source;
  });
}

// === PRIVATE HELPERS ===

/** Pictures that have already loaded once: where they are and how big (the browser keeps the bytes). */
const loaded = new Map<string, { source: string; width: number; height: number }>();

const waiting = new Map<string, ((path: string) => void)[]>();
let listening = false;

/** Bumped when a picked cover replaces the file of the same name; 0 = never (the address stays as is). */
let pickedVersion = 0;

/** A playlist's picked cover is always `cover.jpg`: a new address makes the webview read the new bytes. */
export function refreshPickedCovers(): void {
  pickedVersion = Date.now();
}

async function sourceOf(picture: Picture): Promise<string | null> {
  if (picture.kind === 'file') {
    const url = api.fileUrl(picture.path);
    return pickedVersion ? `${url}${url.includes('?') ? '&' : '?'}v=${pickedVersion}` : url;
  }
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

/** A playlist's own hue (0–359), the same for the same title: the colour behind its header. */
export function hueOf(text: string): number {
  let hash = 0;
  for (const char of text) hash = (hash * 31 + (char.codePointAt(0) ?? 0)) % 360;
  return hash;
}
