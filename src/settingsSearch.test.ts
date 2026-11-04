import { describe, expect, it } from 'vitest';
import { es } from './i18n';
import { buildIndex, searchIndex, type SettingEntry } from './settingsSearch';
import { normalizeForSearch } from './search';

// Port of the Java app's SettingsSearchIndexTest.

/** A fake dictionary with the SAME key patterns as the real one. */
const dictionary: Record<string, string> = {
  // real settings (title + .desc)
  'settings.general.includeIds': 'Incluir IDs en el nombre',
  'settings.general.includeIds.desc': 'Añade el identificador al final.',
  'settings.interface.language': 'Idioma',
  'settings.interface.language.desc': 'Idioma de la aplicación.',
  'settings.updates.auto': 'Actualizaciones automáticas',
  'settings.updates.auto.desc': 'Comprueba una vez al día si hay versión nueva.',
  'settings.advanced.cookies': 'Cookies de YouTube',
  'settings.advanced.cookies.desc': 'Necesario para playlists grandes.',
  // NOT settings
  'settings.title': 'Configuración',
  'settings.general.title': 'General',
  'settings.general.subtitle': 'Configuración general',
  'settings.nav.general': 'General',
  'settings.advanced.cookies.placeholder': 'Ruta al archivo...',
  'settings.advanced.scan': 'Analizar',
  'common.back': 'Atrás',
};

const keys = (entries: SettingEntry[]) => entries.map(e => e.key);

describe('settings search index', () => {
  it('only actual settings get in: a title WITH its description', () => {
    const entries = buildIndex(dictionary);
    expect(entries).toHaveLength(4);
    expect(entries.some(e => e.key.endsWith('.title') || e.key.endsWith('.placeholder') || e.key.startsWith('settings.nav.'))).toBe(false);
    // Fits the pattern but has no .desc: a button's text, not a setting.
    expect(entries.some(e => e.key === 'settings.advanced.scan')).toBe(false);
  });

  it('derives the category from the key', () => {
    const cookies = buildIndex(dictionary).find(e => e.key === 'settings.advanced.cookies');
    expect(cookies).toEqual({ category: 'advanced', key: 'settings.advanced.cookies', title: 'Cookies de YouTube', description: 'Necesario para playlists grandes.' });
  });

  it('regression: the order is the tabs\', not the dictionary\'s', () => {
    const reversed = Object.fromEntries(Object.entries(dictionary).reverse());
    expect(buildIndex(reversed).map(e => e.category)).toEqual(['general', 'interface', 'updates', 'advanced']);
  });

  it('searches by title and by description', () => {
    const all = buildIndex(dictionary);
    expect(keys(searchIndex(all, 'cookies'))).toEqual(['settings.advanced.cookies']);
    // "playlists" only appears in the cookies explanation
    expect(keys(searchIndex(all, 'playlists'))).toEqual(['settings.advanced.cookies']);
  });

  it('neither case nor accents: "automaticas" has to find "automáticas"', () => {
    const all = buildIndex(dictionary);
    expect(keys(searchIndex(all, 'automaticas'))).toEqual(['settings.updates.auto']);
    expect(keys(searchIndex(all, 'AUTOMÁTICAS'))).toEqual(['settings.updates.auto']);
    expect(keys(searchIndex(all, 'IDIOMA'))).toEqual(['settings.interface.language']);
    expect(normalizeForSearch('  Configuración  ')).toBe('configuracion');
  });

  it('a title match ranks above one that only mentions it', () => {
    const all = buildIndex({ ...dictionary, 'settings.general.other': 'Otro', 'settings.general.other.desc': 'Habla de incluir algo.' });
    expect(keys(searchIndex(all, 'incluir'))).toEqual(['settings.general.includeIds', 'settings.general.other']);
  });

  it('an empty search returns nothing', () => {
    const all = buildIndex(dictionary);
    expect(searchIndex(all, '')).toEqual([]);
    expect(searchIndex(all, '   ')).toEqual([]);
    expect(searchIndex(all, null)).toEqual([]);
    expect(searchIndex(all, 'esto no existe en ningun ajuste')).toEqual([]);
  });

  it('copes with what is not there', () => {
    expect(buildIndex(null)).toEqual([]);
    expect(searchIndex(null, 'algo')).toEqual([]);
  });

  it('against the REAL dictionary: if someone renames the keys, the search goes empty', () => {
    const entries = buildIndex(es);
    expect(entries.length).toBeGreaterThanOrEqual(8);
    expect(entries.some(e => e.key === 'settings.advanced.cookies')).toBe(true);
    expect(entries.every(e => e.title.trim() !== '' && e.description.trim() !== '')).toBe(true);
  });
});
