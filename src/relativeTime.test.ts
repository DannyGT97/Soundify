import { describe, expect, it } from 'vitest';
import { ago } from './relativeTime';

// The Java app's RelativeTimeTest (the back sends the days already counted).

describe('ago', () => {
  it('today and yesterday', () => {
    expect(ago(0).key).toBe('player.added.today');
    expect(ago(1).key).toBe('player.added.yesterday');
  });

  it('days, then weeks', () => {
    expect(ago(6)).toEqual({ key: 'player.added.days', amount: 6 });
    expect(ago(7)).toEqual({ key: 'player.added.week', amount: 1 });
    expect(ago(13)).toEqual({ key: 'player.added.week', amount: 1 });
    expect(ago(14)).toEqual({ key: 'player.added.weeks', amount: 2 });
    expect(ago(29)).toEqual({ key: 'player.added.weeks', amount: 4 });
  });

  it('months, then years', () => {
    expect(ago(30)).toEqual({ key: 'player.added.month', amount: 1 });
    expect(ago(364)).toEqual({ key: 'player.added.months', amount: 11 });
    expect(ago(365)).toEqual({ key: 'player.added.year', amount: 1 });
    expect(ago(800)).toEqual({ key: 'player.added.years', amount: 2 });
  });

  it('a date in the future (a clock set wrong) counts as today, never as a negative', () => {
    expect(ago(-3).key).toBe('player.added.today');
  });
});
