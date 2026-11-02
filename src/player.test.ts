import { describe, expect, it } from 'vitest';
import { formatTime } from './player';

describe('formatTime', () => {
  it('shows minutes and two-digit seconds', () => {
    expect(formatTime(0)).toBe('0:00');
    expect(formatTime(187)).toBe('3:07');
  });

  it('adds the hours past the hour', () => {
    expect(formatTime(3765)).toBe('1:02:45');
  });

  it('never goes negative and drops the fraction', () => {
    expect(formatTime(-5)).toBe('0:00');
    expect(formatTime(59.9)).toBe('0:59');
  });
});
