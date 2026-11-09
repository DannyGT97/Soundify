// Where the floating video panel may sit (pure, the drawing is `components/pip.ts`).

export interface Point { x: number; y: number }
export interface Size { width: number; height: number }
export interface Box extends Point, Size {}

export const PIP_SIZE: Size = { width: 320, height: 180 };
/** A phone is 360 px wide: the same panel would cover half of it. */
export const PIP_SIZE_PHONE: Size = { width: 208, height: 117 };
const MARGIN = 8;
/** Clear of the player bar on arrival. */
const START_BOTTOM = 104;
/** On a phone the tab bar sits under the player bar. */
const START_BOTTOM_PHONE = 150;

/** The panel's size for this screen. */
export function pipSize(phone: boolean): Size {
  return phone ? PIP_SIZE_PHONE : PIP_SIZE;
}

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
export function startSpot(panel: Size, view: Size, phone = false): Point {
  const bottom = phone ? START_BOTTOM_PHONE : START_BOTTOM;
  return clampPanel({ x: view.width - panel.width - 24, y: view.height - panel.height - bottom }, panel, view);
}

/**
 * Is the panel held over the slot Reproduciendo keeps for it? Its centre inside
 * the slot: letting go there puts the video back (the slot says so while it is
 * over it).
 */
export function overDock(panel: Box, dock: Box): boolean {
  const x = panel.x + panel.width / 2;
  const y = panel.y + panel.height / 2;
  return x >= dock.x && x <= dock.x + dock.width && y >= dock.y && y <= dock.y + dock.height;
}
