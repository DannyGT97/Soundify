import { describe, expect, it } from 'vitest';
import { PlayerQueue } from './playerQueue';

// The Java app's PlayerQueueTest.

const TRACKS = ['a', 'b', 'c', 'd'];

/** A fixed sequence in [0, 1), so a shuffle is the same on every run. */
function seeded(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state * 1103515245 + 12345) % 2147483648;
    return state / 2147483648;
  };
}

describe('PlayerQueue', () => {
  it('starts where it was asked and goes in order', () => {
    const q = new PlayerQueue(TRACKS, 1);
    expect(q.current()).toBe('b');
    expect(q.next()).toBe('c');
    expect(q.next()).toBe('d');
  });

  it('at the end of the list it stops by itself, but the next button wraps around', () => {
    expect(new PlayerQueue(TRACKS, 3).afterEnd()).toBeNull();
    expect(new PlayerQueue(TRACKS, 3).next()).toBe('a');
  });

  it('previous wraps to the end', () => {
    expect(new PlayerQueue(TRACKS, 0).previous()).toBe('d');
  });

  it('repeat all goes back to the start; repeat one plays the same again, but the button still moves on', () => {
    const q = new PlayerQueue(TRACKS, 3);
    q.repeat = 'all';
    expect(q.afterEnd()).toBe('a');
    q.repeat = 'one';
    expect(q.afterEnd()).toBe('a');
    expect(q.next()).toBe('b');
  });

  it('the repeat button cycles', () => {
    const q = new PlayerQueue(TRACKS, 0);
    expect(q.cycleRepeat()).toBe('all');
    expect(q.cycleRepeat()).toBe('one');
    expect(q.cycleRepeat()).toBe('off');
  });

  it('shuffle keeps the current file playing and plays every other one once before stopping', () => {
    for (const seed of [1, 7, 42, 99]) {
      const q = new PlayerQueue(TRACKS, 2, seeded(seed));
      q.setShuffled(true);
      expect(q.current()).toBe('c');
      const played: string[] = [];
      for (let t = q.afterEnd(); t !== null; t = q.afterEnd()) played.push(t);
      expect(played).toHaveLength(TRACKS.length - 1);
      expect(new Set(['c', ...played])).toEqual(new Set(TRACKS));
    }
  });

  it('previous under shuffle goes back to what really played, and turning it off keeps the file', () => {
    const q = new PlayerQueue(TRACKS, 0, seeded(7));
    q.setShuffled(true);
    const first = q.current();
    const second = q.next();
    expect(q.previous()).toBe(first);
    q.next();
    q.setShuffled(false);
    expect(q.current()).toBe(second);
    expect(q.currentIndex()).toBe(TRACKS.indexOf(second));
  });

  it('jumping to a row plays it', () => {
    const q = new PlayerQueue(TRACKS, 0, seeded(1));
    q.setShuffled(true);
    expect(q.jumpTo(3)).toBe('d');
    expect(q.currentIndex()).toBe(3);
  });

  it('an empty queue is refused', () => {
    expect(() => new PlayerQueue([], 0)).toThrow(RangeError);
  });
});
