import { describe, expect, it } from 'vitest';
import { clampToStep, DEFAULT_SCALE, stepFrom } from './scale';

describe('clampToStep', () => {
  it('snaps a hand-edited value to the nearest step', () => {
    expect(clampToStep(104)).toBe(100);
    expect(clampToStep(118)).toBe(125);
  });

  it('keeps values outside the list inside it', () => {
    expect(clampToStep(10)).toBe(80);
    expect(clampToStep(400)).toBe(175);
  });

  it('falls back to the default for a value that is not a number', () => {
    expect(clampToStep(Number.NaN)).toBe(DEFAULT_SCALE);
  });
});

describe('stepFrom', () => {
  it('moves one step in each direction', () => {
    expect(stepFrom(100, 1)).toBe(110);
    expect(stepFrom(100, -1)).toBe(90);
  });

  it('stops at both ends', () => {
    expect(stepFrom(175, 1)).toBe(175);
    expect(stepFrom(80, -1)).toBe(80);
  });
});
