import { describe, expect, it } from 'vitest';
import { crop, placement, playlistPicture, queuePicture } from './covers';
import type { FormatBlock, Playlist } from './api';

function block(firstVideoId: string | null): FormatBlock {
  return { key: 'k', playlistId: 'p', format: 'MP3', video: false, numbered: false, sync: true, lastSync: '', firstVideoId, tracks: [] };
}

describe('crop', () => {
  it('leaves out the black bands of a YouTube thumbnail before cutting the square', () => {
    expect(crop(480, 360, true, 1)).toEqual({ x: 105, y: 45, width: 270, height: 270 });
  });

  it('cuts the centre of any other picture', () => {
    expect(crop(480, 360, false, 1)).toEqual({ x: 60, y: 0, width: 360, height: 360 });
    expect(crop(300, 500, false, 1)).toEqual({ x: 0, y: 100, width: 300, height: 300 });
  });

  it('fills a 4:3 tile with the whole 16:9 band', () => {
    expect(crop(480, 360, true, 4 / 3)).toEqual({ x: 60, y: 45, width: 360, height: 270 });
  });
});

describe('placement', () => {
  it('scales and shifts the picture so that only the cut shows', () => {
    expect(placement(480, 360, { x: 105, y: 45, width: 270, height: 270 }))
      .toEqual({ left: '-38.89%', top: '-16.67%', width: '177.78%', height: '133.33%' });
  });
});

describe('playlistPicture', () => {
  const playlist = (cover: string | null, ...blocks: FormatBlock[]): Playlist => ({ id: 'p', title: 'P', folder: '', cover, formats: blocks });

  it('prefers the picked cover', () => {
    expect(playlistPicture(playlist('/a/cover.jpg', block('dQw4w9WgXcQ')))).toEqual({ kind: 'file', path: '/a/cover.jpg' });
  });

  it("falls back to the first video's thumbnail, of the first format that has one", () => {
    expect(playlistPicture(playlist(null, block(null), block('dQw4w9WgXcQ')))).toEqual({ kind: 'thumbnail', id: 'dQw4w9WgXcQ' });
  });

  it('has nothing for a playlist with neither', () => {
    expect(playlistPicture(playlist(null, block(null)))).toBeNull();
  });
});

describe('queuePicture', () => {
  it("is the thumbnail of the remote playlist's first file", () => {
    expect(queuePicture({ items: [{ videoId: 'dQw4w9WgXcQ' }, { videoId: 'x' }] })).toEqual({ kind: 'thumbnail', id: 'dQw4w9WgXcQ' });
  });

  it('is nothing while the playlist is not known or empty', () => {
    expect(queuePicture(null)).toBeNull();
    expect(queuePicture({ items: [] })).toBeNull();
  });
});
