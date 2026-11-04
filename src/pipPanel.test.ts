import { describe, expect, it } from 'vitest';
import { clampPanel, PIP_SIZE, startSpot } from './pipPanel';

const view = { width: 1280, height: 820 };

describe('the floating panel stays in the window', () => {
  it('leaves alone a spot that fits', () => {
    expect(clampPanel({ x: 100, y: 200 }, PIP_SIZE, view)).toEqual({ x: 100, y: 200 });
  });

  it('pulls it back from every edge', () => {
    expect(clampPanel({ x: -50, y: -50 }, PIP_SIZE, view)).toEqual({ x: 8, y: 8 });
    expect(clampPanel({ x: 5000, y: 5000 }, PIP_SIZE, view)).toEqual({ x: 952, y: 632 });
  });

  it('in a window smaller than the panel, the top left corner wins', () => {
    expect(clampPanel({ x: 200, y: 200 }, PIP_SIZE, { width: 100, height: 100 })).toEqual({ x: 8, y: 8 });
  });

  it('does not hide under the title bar', () => {
    expect(clampPanel({ x: 100, y: 0 }, PIP_SIZE, view, 36)).toEqual({ x: 100, y: 44 });
  });

  it('starts at the bottom right, clear of the player bar', () => {
    const spot = startSpot(PIP_SIZE, view);
    expect(spot.x + PIP_SIZE.width).toBeLessThan(view.width);
    expect(spot.y + PIP_SIZE.height).toBeLessThanOrEqual(view.height - 100);
  });
});
