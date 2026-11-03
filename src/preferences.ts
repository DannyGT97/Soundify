// The preferences in settings.json: read once before the first screen and
// applied (look, language, scale, player placement); from then on, every
// change made in Ajustes is applied on the spot AND saved — like the Java
// panels, there is no «Guardar» button.

import { api, type Preferences, type PreferencesPatch } from './api';
import { setLanguage } from './i18n';
import { initScale, onScaleChange, setScale } from './scale';
import { PLAYER_PLACEMENTS, THEMES, setPlayerPlacement, setTheme, type PlayerPlacement, type Theme } from './state';

let prefs: Preferences | null = null;

export function preferences(): Preferences {
  if (!prefs) throw new Error('preferences read before loadPreferences()');
  return prefs;
}

/** Once, before the first screen: nothing is drawn in the wrong look or language. */
export async function loadPreferences(): Promise<void> {
  prefs = await api.preferences();
  applyAll(prefs);
  initScale(prefs.uiScale);
  onScaleChange(uiScale => save({ uiScale }));
}

/** Writes a change; whoever calls has already applied it. */
export function save(patch: PreferencesPatch): void {
  if (prefs) prefs = { ...prefs, ...patch };
  api.savePreferences(patch).catch((e: unknown) => console.warn('Could not save the settings', e));
}

export function chooseTheme(theme: Theme): void {
  setTheme(theme);
  save({ appearance: theme });
}

export function choosePlayerPlacement(value: PlayerPlacement): void {
  setPlayerPlacement(value);
  save({ playerPlacement: value });
}

export function chooseLanguage(code: string): void {
  save({ language: code });
  setLanguage(code);
}

/**
 * Ajustes → Avanzado → Reset. Like the Java app, it is applied on the spot:
 * writing the file alone changed nothing that could be seen, and the button
 * seemed to do nothing.
 */
export async function resetPreferences(): Promise<void> {
  prefs = await api.resetPreferences();
  applyAll(prefs);
  setScale(prefs.uiScale);
}

function applyAll(p: Preferences): void {
  setTheme(THEMES.includes(p.appearance as Theme) ? p.appearance as Theme : 'dark');
  setPlayerPlacement(PLAYER_PLACEMENTS.includes(p.playerPlacement as PlayerPlacement) ? p.playerPlacement as PlayerPlacement : 'both');
  setLanguage(p.language);
}
