/// <reference types="vite/client" />
import { describe, expect, it } from 'vitest';
import { DICTIONARIES, hasKey, keys } from './i18n';

// The front's twin of the Java app's I18nKeysTest: a key used in the code and
// missing from the dictionary shows the raw key on screen, and nothing else fails.

const rustSources = import.meta.glob<string>(['../core/src/**/*.rs', '../src-tauri/src/**/*.rs'], { query: '?raw', import: 'default', eager: true });
const sources = import.meta.glob<string>(['./**/*.ts', '!./**/*.test.ts'], { query: '?raw', import: 'default', eager: true });

function matches(pattern: RegExp): { file: string; key: string }[] {
  return Object.entries(sources).flatMap(([file, text]) =>
    [...text.matchAll(pattern)].map(m => ({ file, key: m[1] ?? '' })));
}

describe('i18n keys', () => {
  it('every literal key passed to t() exists', () => {
    const used = matches(/\bt\(\s*'([^']+)'/g);
    // Guards the test itself: a broken glob or pattern would find nothing and pass.
    expect(used.length).toBeGreaterThan(100);
    expect(used.filter(({ key }) => !hasKey(key))).toEqual([]);
  });

  it('every key built from a template has at least one key with its prefix', () => {
    const prefixes = matches(/\bt\(\s*`([^`$]+)\$\{/g);
    const all = keys();
    const missing = prefixes.filter(({ key }) => !all.some(k => k.startsWith(key)));
    expect(missing).toEqual([]);
  });

  it('every key named in a menu or label table exists', () => {
    // Keys stored in tables and translated later (the menu's entries, settings tabs).
    const named = matches(/\b(?:label|labelKey|titleKey|captionKey|key):\s*'([a-z][\w-]*(?:\.[\w-]+)+)'/g);
    expect(named.length).toBeGreaterThan(0);
    expect(named.filter(({ key }) => !hasKey(key))).toEqual([]);
  });

  it('the three languages carry the same keys, none of them empty', () => {
    // A key only in Spanish would show Spanish in the middle of an English screen.
    const reference = Object.keys(DICTIONARIES.es).sort();
    for (const [language, dictionary] of Object.entries(DICTIONARIES)) {
      expect(Object.keys(dictionary).sort(), language).toEqual(reference);
      expect(Object.entries(dictionary).filter(([, text]) => !text.trim()).map(([key]) => key), language).toEqual([]);
    }
  });

  it('a translation keeps the {n} placeholders of the Spanish text', () => {
    const placeholders = (text: string) => [...text.matchAll(/\{\d\}/g)].map(m => m[0]).sort().join();
    for (const [language, dictionary] of Object.entries(DICTIONARIES)) {
      const broken = Object.keys(DICTIONARIES.es).filter(key => placeholders(dictionary[key] ?? '') !== placeholders(DICTIONARIES.es[key] ?? ''));
      expect(broken, language).toEqual([]);
    }
  });

  it('every key the back hands over exists', () => {
    // The back names keys too (error messages, the tools notice). A Rust string
    // literal in one of the dictionary's namespaces has to be a real key.
    const all = keys();
    const namespaces = new Set(all.map(k => k.split('.')[0]));
    const named = Object.entries(rustSources).flatMap(([file, text]) =>
      [...text.matchAll(/"([a-z][A-Za-z]*\.[A-Za-z][\w.]*)"/g)].map(m => ({ file, key: m[1] ?? '' })))
      // File names ("settings.json") share a namespace's first word.
      .filter(({ key }) => namespaces.has(key.split('.')[0] ?? '') && !/\.(json|txt|exe|zip|jpg)$/.test(key));
    expect(named.some(({ key }) => key === 'toolHealth.willFail')).toBe(true);
    expect(named.filter(({ key }) => !hasKey(key))).toEqual([]);
  });
});
