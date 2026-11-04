// Where the floating video panel may sit (pure, the drawing is `components/pip.ts`).

export interface Point { x: number; y: number }
export interface Size { width: number; height: number }

export const PIP_SIZE: Size = { width: 320, height: 180 };
const MARGIN = 8;
/** Clear of the player bar on arrival. */
const START_BOTTOM = 104;

/**
 * Keeps the whole panel inside the window and below its title bar (`top`: it
 * would hide under it); in a window too small for it, the top left corner wins.
 */
export function clampPanel(at: Point, panel: Size, view: Size, top = 0): Point {
  const fit = (value: number, length: number, room: number, from: number) =>
    Math.max(from + MARGIN, Math.min(value, room - length - MARGIN));
  return { x: fit(at.x, panel.width, view.width, 0), y: fit(at.y, panel.height, view.height, top) };
}

/** Bottom right, the first time it opens. */
export function startSpot(panel: Size, view: Size): Point {
  return clampPanel({ x: view.width - panel.width - 24, y: view.height - panel.height - START_BOTTOM }, panel, view);
}
