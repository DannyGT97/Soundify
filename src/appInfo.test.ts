import { describe, expect, it } from 'vitest';
import { versionLabel } from './appInfo';
import { t } from './i18n';

describe('versionLabel', () => {
  it('shows the version with its v', () => {
    expect(versionLabel('1.4.0')).toBe('v1.4.0');
    expect(versionLabel(' v0.1.0 ')).toBe('v0.1.0');
    expect(versionLabel('2.0.0-beta.1')).toBe('v2.0.0-beta.1');
  });

  it('regression: never shows null or an unfiltered placeholder', () => {
    // The Java app once showed "null" and "${project.version}" in «Acerca de».
    for (const raw of [null, undefined, '', '   ', 'null', '${project.version}']) {
      expect(versionLabel(raw)).toBe(t('app.unknownVersion'));
    }
  });
});
