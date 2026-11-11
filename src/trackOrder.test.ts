import { describe, expect, it } from 'vitest';
import { DEFAULT_ORDER, nextOrder, orderOf, setOrder, sortTracks } from './trackOrder';

const track = (number: number, title: string, addedDaysAgo: number) => ({ number, title, addedDaysAgo });
const files = [track(1, 'Zeta', 5), track(2, 'álamo', 0), track(3, 'Beta 10', 9), track(4, 'Beta 2', 5)];
const numbers = (order: Parameters<typeof sortTracks>[1]) => sortTracks(files, order).map(t => t.number);

describe('sortTracks', () => {
  it('by number, in both directions', () => {
    expect(numbers({ key: 'number', dir: 'asc' })).toEqual([1, 2, 3, 4]);
    expect(numbers({ key: 'number', dir: 'desc' })).toEqual([4, 3, 2, 1]);
  });

  it('by title ignoring accents and case, and counting numbers as numbers', () => {
    expect(numbers({ key: 'title', dir: 'asc' })).toEqual([2, 4, 3, 1]);
    expect(numbers({ key: 'title', dir: 'desc' })).toEqual([1, 3, 4, 2]);
  });

  it('by download time: ascending is the most recent first, ties by number', () => {
    expect(numbers({ key: 'added', dir: 'asc' })).toEqual([2, 1, 4, 3]);
    expect(numbers({ key: 'added', dir: 'desc' })).toEqual([3, 1, 4, 2]);
  });

  it('does not touch the list it is given', () => {
    const before = files.map(t => t.number);
    sortTracks(files, { key: 'title', dir: 'desc' });
    expect(files.map(t => t.number)).toEqual(before);
  });
});

describe('nextOrder', () => {
  it('flips the active column and starts the others ascending', () => {
    expect(nextOrder(DEFAULT_ORDER, 'number')).toEqual({ key: 'number', dir: 'desc' });
    expect(nextOrder({ key: 'number', dir: 'desc' }, 'title')).toEqual({ key: 'title', dir: 'asc' });
    expect(nextOrder({ key: 'title', dir: 'asc' }, 'title')).toEqual({ key: 'title', dir: 'desc' });
  });
});

describe('the order each playlist was left in', () => {
  it('defaults to the number and is kept per playlist', () => {
    expect(orderOf('pl1:MP3')).toEqual(DEFAULT_ORDER);
    setOrder('pl1:MP3', { key: 'added', dir: 'asc' });
    expect(orderOf('pl1:MP3')).toEqual({ key: 'added', dir: 'asc' });
    expect(orderOf('pl2:MP3')).toEqual(DEFAULT_ORDER);
  });
});
