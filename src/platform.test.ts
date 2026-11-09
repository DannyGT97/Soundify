import { describe, expect, it } from 'vitest';
import { keyboardOpen } from './platform';

describe('keyboardOpen', () => {
  it('needs a text field with the focus and a page that lost height', () => {
    expect(keyboardOpen(true, 330, 800)).toBe(true);
    expect(keyboardOpen(false, 330, 800)).toBe(false);
    expect(keyboardOpen(true, 800, 800)).toBe(false);
  });
  it('ignores the small changes of the system bars', () => {
    expect(keyboardOpen(true, 720, 800)).toBe(false);
  });
});
