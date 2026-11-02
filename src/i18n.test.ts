/// <reference types="vite/client" />
import { describe, expect, it } from 'vitest';
import { hasKey, keys } from './i18n';

// The front's twin of the Java app's I18nKeysTest: a key used in the code and
// missing from the dictionary shows the raw key on screen, and nothing else fails.

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
});
