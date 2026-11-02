import { describe, expect, it } from 'vitest';
import type { Playlist, Status, Track } from './api';
import { hasKey } from './i18n';
import { countBy, findBlock, onDisk, statusText } from './library';

function track(id: string, status: Status): Track {
  return { id, number: 1, title: id, addedDaysAgo: 0, seconds: 100, status };
}

const tracks = [track('a', 'ok'), track('b', 'pending'), track('c', 'ok'), track('d', 'unavailable')];

const playlists: Playlist[] = [{
  id: 'pl1', title: 'One', folder: 'D:\\Music\\One',
  formats: [
    { key: 'pl1:MP3', playlistId: 'pl1', format: 'MP3', video: false, numbered: true, sync: true, lastSync: '', tracks },
    { key: 'pl1:FLAC', playlistId: 'pl1', format: 'FLAC', video: false, numbered: false, sync: false, lastSync: '', tracks: [] },
  ],
}];

describe('library helpers', () => {
  it('counts the tracks in one state', () => {
    expect(countBy(tracks, 'ok')).toBe(2);
    expect(countBy(tracks, 'failed')).toBe(0);
  });

  it('keeps only the files on disk, in their order', () => {
    expect(onDisk(tracks).map(tr => tr.id)).toEqual(['a', 'c']);
  });

  it('finds a block by its key, with its playlist', () => {
    const found = findBlock('pl1:FLAC', playlists);
    expect(found?.playlist.id).toBe('pl1');
    expect(found?.block.format).toBe('FLAC');
  });

  it('finds nothing for an unknown or empty key', () => {
    expect(findBlock('pl9:MP3', playlists)).toBeNull();
    expect(findBlock(null, playlists)).toBeNull();
  });
});

describe('statusText', () => {
  it('an unavailable file says why', () => {
    expect(statusText({ ...track('x', 'unavailable'), reason: 'unavailable.privateVideo' }))
      .toBe('No disponible, vídeo privado');
  });

  it('with no reason, or only the generic one, it is the plain status', () => {
    expect(statusText(track('x', 'unavailable'))).toBe('No disponible');
    expect(statusText({ ...track('x', 'unavailable'), reason: 'unavailable.generic' })).toBe('No disponible');
  });

  it('every reason the back can send has its text', () => {
    for (const key of ['accountTerminated', 'copyright', 'privateVideo', 'removedByUploader', 'membersOnly',
                       'ageRestricted', 'geoBlocked', 'liveEnded', 'generic']) {
      expect(hasKey(`unavailable.${key}`), key).toBe(true);
    }
  });

  it('a reason only counts on an unavailable file', () => {
    expect(statusText({ ...track('x', 'ok'), reason: 'unavailable.privateVideo' })).toBe('OK');
  });
});
