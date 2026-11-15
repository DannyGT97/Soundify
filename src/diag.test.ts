import { describe, expect, it } from 'vitest';
import { createDiag, type Level } from './diag';

function setup() {
  let at = 1_000_000;
  const lines: Array<[Level, string, string]> = [];
  const diag = createDiag((level, area, message) => lines.push([level, area, message]), () => at);
  return { diag, lines, advance: (ms: number) => { at += ms; } };
}

describe('the page log', () => {
  it('writes an event once and counts the same one repeated at once', () => {
    const { diag, lines, advance } = setup();
    diag.logEvent('info', 'connect', 'took');
    diag.logEvent('info', 'connect', 'took');
    diag.logEvent('info', 'connect', 'took');
    expect(lines).toHaveLength(1);
    advance(2500);
    diag.logEvent('info', 'connect', 'took');
    expect(lines).toHaveLength(2);
    expect(lines[1]?.[2]).toContain('x3 just before');
  });

  it('cannot flood the file: past the limit lines are held back and said so', () => {
    const { diag, lines, advance } = setup();
    for (let i = 0; i < 100; i++) diag.logEvent('info', 'loop', `line ${i}`);
    expect(lines.length).toBeLessThanOrEqual(21);
    advance(1500);
    diag.logEvent('info', 'loop', 'later');
    expect(lines.at(-1)?.[2]).toContain('lines held back');
  });

  it('sums the counters in one line and warns when one looks like a storm', () => {
    const { diag } = setup();
    diag.count('publish', 5);
    diag.count('loadConnect', 3);
    expect(diag.summary(30)).toEqual({ level: 'info', text: 'stats (30s): loadConnect=3 publish=5' });
    expect(diag.summary(30)).toBeNull();
    diag.count('publish', 500);
    const storm = diag.summary(30);
    expect(storm?.level).toBe('warn');
    expect(storm?.text).toContain('more than usual: publish');
  });
});
