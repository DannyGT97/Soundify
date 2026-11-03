// The catalogue of settings the Ajustes search box looks through.
//
// It comes from the LANGUAGE KEYS, not from walking the panels: a panel is only
// built when its tab opens, and building them all just to search would have real
// effects (the Updates one asks the network). With the keys everything is
// indexed without opening a panel, and the search runs in the language in use.
//
// What counts as a setting: a `settings.<category>.<something>` key that also
// has its `.desc`. That pair is exactly what paints a setting card (title +
// explanation), so no list has to be kept by hand. It leaves out what is not a
// setting: category titles, subtitles, tab names, button texts, placeholders.

import { normalizeForSearch } from './search';

/** Categories with a panel of their own, in the same order as the tabs. */
export const CATEGORIES = ['general', 'interface', 'updates', 'tools', 'advanced'] as const;
export type Category = typeof CATEGORIES[number];

export interface SettingEntry {
  /** Which tab to open it with. */
  category: Category;
  key: string;
  title: string;
  description: string;
}

const DESC_SUFFIX = '.desc';

/** Every setting in the dictionary, in the order of the tabs and then by title. */
export function buildIndex(dictionary: Record<string, string> | null | undefined): SettingEntry[] {
  if (!dictionary) return [];
  const entries: SettingEntry[] = [];
  for (const [key, title] of Object.entries(dictionary)) {
    const category = categoryOf(key);
    const description = dictionary[key + DESC_SUFFIX];
    if (!category || description === undefined) continue;
    entries.push({ category, key, title, description });
  }
  // Sorted, so the results never depend on how the dictionary was written.
  return entries.sort((a, b) =>
    CATEGORIES.indexOf(a.category) - CATEGORIES.indexOf(b.category) || a.title.localeCompare(b.title));
}

/**
 * The settings matching `query`: first by title, then the ones that only
 * mention it in their explanation — whoever types "idioma" expects the setting
 * called that before another one that names it in passing. Blank finds nothing.
 */
export function searchIndex(entries: readonly SettingEntry[] | null | undefined, query: string | null | undefined): SettingEntry[] {
  const needle = normalizeForSearch(query);
  if (!entries || !needle) return [];
  const byTitle: SettingEntry[] = [];
  const byDescription: SettingEntry[] = [];
  for (const entry of entries) {
    if (normalizeForSearch(entry.title).includes(needle)) byTitle.push(entry);
    else if (normalizeForSearch(entry.description).includes(needle)) byDescription.push(entry);
  }
  return [...byTitle, ...byDescription];
}

/**
 * "settings.advanced.cookies" → "advanced". Null when the key is not a
 * setting's: exactly three parts, a known category (that rules out ".desc",
 * ".placeholder", ".button"…).
 */
function categoryOf(key: string): Category | null {
  const parts = key.split('.');
  if (parts.length !== 3 || parts[0] !== 'settings') return null;
  return (CATEGORIES as readonly string[]).includes(parts[1] ?? '') ? parts[1] as Category : null;
}
