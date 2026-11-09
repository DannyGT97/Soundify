import { describe, expect, it } from 'vitest';
import { clampPanel, overDock, PIP_SIZE, pipSize, startSpot } from './pipPanel';

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

  it('is smaller on a phone and starts above its two bars', () => {
    const phone = { width: 360, height: 780 };
    const size = pipSize(true);
    expect(size.width).toBeLessThan(phone.width / 2 + 40);
    expect(pipSize(false)).toBe(PIP_SIZE);
    const spot = startSpot(size, phone, true);
    expect(spot.y + size.height).toBeLessThanOrEqual(phone.height - 140);
    expect(spot.x).toBeGreaterThanOrEqual(8);
  });
});

describe('letting the panel go over its slot', () => {
  const dock = { x: 300, y: 100, width: 400, height: 225 };

  it('counts when the panel centre is inside the slot', () => {
    expect(overDock({ x: 400, y: 150, ...PIP_SIZE }, dock)).toBe(true);
  });

  it('does not count when only a corner touches it', () => {
    // Overlapping, but the centre (x: 280 + 160 = 440... y: -60 + 90 = 30) is above the slot.
    expect(overDock({ x: 280, y: -60, ...PIP_SIZE }, dock)).toBe(false);
  });

  it('does not count far away', () => {
    expect(overDock({ x: 900, y: 600, ...PIP_SIZE }, dock)).toBe(false);
  });
});
