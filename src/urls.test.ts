import { describe, expect, it } from 'vitest';
import { isPlaylistUrl, isWebUrl } from './urls';

describe('isWebUrl', () => {
  it('accepts http and https addresses, ignoring surrounding spaces', () => {
    expect(isWebUrl('https://www.youtube.com/watch?v=abc')).toBe(true);
    expect(isWebUrl('  http://youtu.be/abc  ')).toBe(true);
  });

  it('rejects text that is not an address', () => {
    expect(isWebUrl('')).toBe(false);
    expect(isWebUrl('youtube.com/watch?v=abc')).toBe(false);
    expect(isWebUrl('https://localhost')).toBe(false);
  });
});

describe('isPlaylistUrl', () => {
  it('accepts a playlist address', () => {
    expect(isPlaylistUrl('https://www.youtube.com/playlist?list=PL123')).toBe(true);
  });

  it('rejects a video opened from inside a playlist: it is a single video', () => {
    expect(isPlaylistUrl('https://www.youtube.com/watch?v=abc&list=PL123')).toBe(false);
  });

  it('rejects a plain video', () => {
    expect(isPlaylistUrl('https://www.youtube.com/watch?v=abc')).toBe(false);
  });
});
