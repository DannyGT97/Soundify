// The interface scale (Ajustes → Interfaz). In the app it is the webview's own
// zoom, the same one as Ctrl + / Ctrl − in a browser: the page is laid out
// again at the new size, so 100vh, scroll bars and the minimum window size keep
// working. CSS `zoom` on the root looks the same at first, but the screen-high
// layout then grows past the window (or leaves a gap below) — that is what
// failed before. In a plain browser (npm run dev) there is no webview to ask,
// so the zoom goes on #app, whose height is a percentage and is not scaled.
//
// The value is settings.json's `uiScale`: `preferences.ts` hands it in at
// startup and saves every change (it listens with `onScaleChange`).

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';

export const SCALE_STEPS = [80, 90, 100, 110, 125, 150, 175] as const;
export const DEFAULT_SCALE = 100;

const listeners = new Set<(percent: number) => void>();
let current = DEFAULT_SCALE;

export function currentScale(): number {
  return current;
}

/** Applies the saved scale; called once, before the first screen is drawn. */
export function initScale(saved: number): void {
  current = clampToStep(saved);
  void apply(current);
  document.addEventListener('keydown', onShortcut);
}

export function setScale(percent: number): void {
  const next = clampToStep(percent);
  if (next === current) return;
  current = next;
  void apply(next);
  listeners.forEach(listener => listener(next));
}

/** Lets the Ajustes list follow a change made with the keyboard. */
export function onScaleChange(listener: (percent: number) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The nearest step, so a hand-edited or old value never leaves the list without a match. */
export function clampToStep(percent: number): number {
  if (!Number.isFinite(percent)) return DEFAULT_SCALE;
  return SCALE_STEPS.reduce((best, step) => Math.abs(step - percent) < Math.abs(best - percent) ? step : best, SCALE_STEPS[0]);
}

/** The step `direction` places away (+1 bigger, −1 smaller), stopping at both ends. */
export function stepFrom(percent: number, direction: 1 | -1): number {
  const index = SCALE_STEPS.indexOf(clampToStep(percent) as (typeof SCALE_STEPS)[number]);
  const next = Math.min(SCALE_STEPS.length - 1, Math.max(0, index + direction));
  return SCALE_STEPS[next]!;
}

// === PRIVATE ===

async function apply(percent: number): Promise<void> {
  const factor = percent / 100;
  if (isTauri()) {
    await getCurrentWebview().setZoom(factor);
    return;
  }
  document.getElementById('app')?.style.setProperty('zoom', String(factor));
}

/** Ctrl + / Ctrl − / Ctrl 0, going through the same steps as the list in Ajustes. */
function onShortcut(e: KeyboardEvent): void {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
  if (e.key === '+' || e.key === '=') setScale(stepFrom(current, 1));
  else if (e.key === '-') setScale(stepFrom(current, -1));
  else if (e.key === '0') setScale(DEFAULT_SCALE);
  else return;
  e.preventDefault();
}
