// The built-in player's queue: what plays next and what came before, with
// shuffle and repeat. Pure logic: `player.ts` plays what this decides.
//
// Shuffle does not pick at random on every "next": it fixes a shuffled ORDER
// once, so "previous" goes back to what really sounded before and nothing
// repeats until everything has played (what people expect from shuffle). The
// file playing right now stays first in that order.

export type Repeat = 'off' | 'all' | 'one';

export class PlayerQueue<T> {
  private readonly items: readonly T[];
  // The play order: positions into items. Identity unless shuffled.
  private order: number[];
  // Where we are in order.
  private cursor: number;
  private shuffledOn = false;
  private repeatMode: Repeat = 'off';

  /** `random` gives [0, 1) like `Math.random`; tests pass a fixed one. */
  constructor(items: readonly T[], startIndex: number, private readonly random: () => number = Math.random) {
    if (items.length === 0) throw new RangeError('An empty queue has nothing to play');
    this.items = [...items];
    this.order = this.identity();
    this.cursor = Math.max(0, Math.min(startIndex, items.length - 1));
  }

  // === PUBLIC API ===

  current(): T {
    return this.items[this.order[this.cursor] as number] as T;
  }

  /** Index of the current file in the ORIGINAL list (for marking its row). */
  currentIndex(): number {
    return this.order[this.cursor] as number;
  }

  get size(): number {
    return this.items.length;
  }

  /**
   * The next file when the current one ENDS on its own: repeat-one plays it
   * again, and at the end of the list it stops (null) unless repeating all.
   */
  afterEnd(): T | null {
    if (this.repeatMode === 'one') return this.current();
    return this.advance(this.repeatMode === 'all');
  }

  /** The "next" button: always moves on (repeat-one only applies to a file ending by itself); wraps around. */
  next(): T {
    return this.advance(true) as T;
  }

  /** The "previous" button: one back, wrapping to the end. */
  previous(): T {
    this.cursor = (this.cursor - 1 + this.order.length) % this.order.length;
    return this.current();
  }

  /** Jumps to a file picked from the list (by its index in the original list). */
  jumpTo(index: number): T {
    this.cursor = Math.max(0, this.order.indexOf(index));
    return this.current();
  }

  get shuffled(): boolean {
    return this.shuffledOn;
  }

  /** Shuffles the rest of the queue around the current file (it keeps playing), or restores the original order. */
  setShuffled(shuffle: boolean): void {
    const playing = this.currentIndex();
    this.shuffledOn = shuffle;
    if (shuffle) {
      const rest = this.identity().filter(i => i !== playing);
      // Fisher–Yates.
      for (let i = rest.length - 1; i > 0; i--) {
        const j = Math.floor(this.random() * (i + 1));
        [rest[i], rest[j]] = [rest[j] as number, rest[i] as number];
      }
      this.order = [playing, ...rest];
      this.cursor = 0;
    } else {
      this.order = this.identity();
      this.cursor = playing;
    }
  }

  get repeat(): Repeat {
    return this.repeatMode;
  }

  set repeat(mode: Repeat) {
    this.repeatMode = mode;
  }

  /** The repeat button cycles off → all → one → off. */
  cycleRepeat(): Repeat {
    this.repeatMode = nextRepeat(this.repeatMode);
    return this.repeatMode;
  }

  // === PRIVATE HELPERS ===

  private advance(wrap: boolean): T | null {
    if (this.cursor + 1 < this.order.length) {
      this.cursor++;
      return this.current();
    }
    if (!wrap) return null;
    this.cursor = 0;
    return this.current();
  }

  private identity(): number[] {
    return this.items.map((_, i) => i);
  }
}

/** off → all → one → off: the button's cycle, also with nothing loaded. */
export function nextRepeat(mode: Repeat): Repeat {
  return mode === 'off' ? 'all' : mode === 'all' ? 'one' : 'off';
}
