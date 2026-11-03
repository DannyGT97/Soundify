import { describe, expect, it } from 'vitest';
import { formatTime, isPlayable } from './player';

// The Java app's PlayerBarControllerTest (the times) and the format probe on
// WebView2 that decided what the built-in player opens.

describe('formatTime', () => {
  it('minutes and seconds, and hours past the hour', () => {
    expect(formatTime(0)).toBe('0:00');
    expect(formatTime(7.9)).toBe('0:07');
    expect(formatTime(59.9)).toBe('0:59');
    expect(formatTime(187)).toBe('3:07');
    expect(formatTime(3765)).toBe('1:02:45');
  });

  it('nonsense is 0:00, never "NaN:NaN"', () => {
    expect(formatTime(Number.NaN)).toBe('0:00');
    expect(formatTime(-1)).toBe('0:00');
    expect(formatTime(Number.POSITIVE_INFINITY)).toBe('0:00');
  });
});

describe('isPlayable', () => {
  it('opens what the webview plays', () => {
    for (const format of ['MP3', 'OPUS', 'M4A', 'FLAC', 'AAC', 'VORBIS', 'WAV', 'MP4', 'MKV', 'MOV', 'WEBM']) {
      expect(isPlayable(format), format).toBe(true);
    }
  });

  it('regression: ALAC is .m4a too but does not play; AVI and FLV do not open', () => {
    expect(isPlayable('ALAC')).toBe(false);
    expect(isPlayable('AVI')).toBe(false);
    expect(isPlayable('FLV')).toBe(false);
  });

  it('by name whatever the case, and nothing for no format', () => {
    expect(isPlayable('webm')).toBe(true);
    expect(isPlayable(null)).toBe(false);
    expect(isPlayable('')).toBe(false);
  });
});
