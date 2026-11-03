import { describe, expect, it } from 'vitest';
import { spaceTypes } from './keys';

describe('spaceTypes', () => {
  it('a space types into text fields, text areas, lists and editable areas', () => {
    expect(spaceTypes({ tagName: 'INPUT', type: 'text' })).toBe(true);
    expect(spaceTypes({ tagName: 'INPUT', type: 'search' })).toBe(true);
    expect(spaceTypes({ tagName: 'INPUT' })).toBe(true);
    expect(spaceTypes({ tagName: 'TEXTAREA' })).toBe(true);
    expect(spaceTypes({ tagName: 'SELECT' })).toBe(true);
    expect(spaceTypes({ tagName: 'DIV', isContentEditable: true })).toBe(true);
  });

  it('regression: on a button, a checkbox or a slider it plays and pauses instead of pressing it again', () => {
    expect(spaceTypes({ tagName: 'BUTTON' })).toBe(false);
    expect(spaceTypes({ tagName: 'INPUT', type: 'checkbox' })).toBe(false);
    expect(spaceTypes({ tagName: 'INPUT', type: 'range' })).toBe(false);
    expect(spaceTypes({ tagName: 'BODY' })).toBe(false);
    expect(spaceTypes(null)).toBe(false);
  });
});
