import { describe, expect, it } from 'vitest';
import { isVideoFile } from './androidPlayer';

describe('which engine plays a file on the phone', () => {
  it('sends video to the webview, which is the only one that can draw it', () => {
    for (const name of ['a.mp4', 'a.MKV', 'a.webm', 'a.mov', 'a.m4v', 'a.avi', 'a.flv']) expect(isVideoFile(`/storage/emulated/0/Music/Soundify/${name}`)).toBe(true);
  });

  it('leaves the music to the native player', () => {
    for (const name of ['a.mp3', 'a.opus', 'a.m4a', 'a.flac', 'a.wav', 'a.ogg', 'no extension']) expect(isVideoFile(`/music/${name}`)).toBe(false);
    expect(isVideoFile(null)).toBe(false);
    expect(isVideoFile(undefined)).toBe(false);
  });
});
