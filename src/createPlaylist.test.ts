import { describe, expect, it } from 'vitest';
import { canCreateFrom, createdNotice, existingPlaylist } from './createPlaylist';
import type { CreatedPlaylist } from './api/types';

const CREATED: CreatedPlaylist = {
  blockKey: 'x', title: 'Lo-fi', count: 12, genericTitle: false, numberMode: { numberFiles: true, source: 'requested' },
};

describe('canCreateFrom', () => {
  it('accepts a playlist, also opened from one of its videos', () => {
    expect(canCreateFrom('https://www.youtube.com/playlist?list=PL1')).toBe(true);
    expect(canCreateFrom('https://www.youtube.com/watch?v=abc&list=PL1')).toBe(true);
  });

  it('rejects a single video', () => {
    expect(canCreateFrom('https://www.youtube.com/watch?v=abc')).toBe(false);
  });
});

describe('createdNotice', () => {
  it('names the playlist and how many videos it listed', () => {
    expect(createdNotice(CREATED)).toEqual({ tone: 'success', text: '"Lo-fi" registrada (12 vídeo(s) listados).' });
  });

  it('warns when only the generic title could be had', () => {
    const notice = createdNotice({ ...CREATED, title: 'soundify_playlist_mp3', genericTitle: true });
    expect(notice.tone).toBe('warning');
    expect(notice.text).toContain('soundify_playlist_mp3');
  });

  it('says so when an earlier registration decided the numbering, not the checkbox', () => {
    const notice = createdNotice({ ...CREATED, numberMode: { numberFiles: false, source: 'sameFormat' } });
    expect(notice.text).toContain('se mantiene su modo (sin numerar)');
    expect(createdNotice({ ...CREATED, numberMode: { numberFiles: true, source: 'otherFormat' } }).text)
      .toContain('otros formatos ya registrados de esta misma playlist y carpeta (numerada)');
  });
});

describe('existingPlaylist', () => {
  it('reads the title and the block key of the already-registered refusal', () => {
    expect(existingPlaylist({ key: 'newPlaylist.alreadyExists', args: ['Lo-fi', 'pl_Lo-fi_abc/mp3/data.json'] }))
      .toEqual({ title: 'Lo-fi', blockKey: 'pl_Lo-fi_abc/mp3/data.json' });
  });

  it('is null for any other error', () => {
    expect(existingPlaylist({ key: 'newPlaylist.queryFailed', args: [] })).toBeNull();
    expect(existingPlaylist(new Error('boom'))).toBeNull();
    expect(existingPlaylist(null)).toBeNull();
  });
});
