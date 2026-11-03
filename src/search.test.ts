import { describe, expect, it } from 'vitest';
import { filterByTitle } from './search';

// The Java app's PlayerControllerTest (the list's search box).

const TRACKS = [
  { title: 'Canción del Mar' },
  { title: 'Dubstep Mix 2024' },
  { title: 'Live or Die' },
];

describe('filterByTitle', () => {
  it('an empty, blank or missing query keeps every file', () => {
    expect(filterByTitle(TRACKS, '')).toEqual(TRACKS);
    expect(filterByTitle(TRACKS, '   ')).toEqual(TRACKS);
    expect(filterByTitle(TRACKS, null)).toEqual(TRACKS);
  });

  it('ignores case, accents and the spaces around the query', () => {
    expect(filterByTitle(TRACKS, 'cancion').map(t => t.title)).toEqual(['Canción del Mar']);
    expect(filterByTitle(TRACKS, ' dubstep ').map(t => t.title)).toEqual(['Dubstep Mix 2024']);
    expect(filterByTitle(TRACKS, 'live or').map(t => t.title)).toEqual(['Live or Die']);
  });

  it('nothing matching gives an empty list', () => {
    expect(filterByTitle(TRACKS, 'phonk')).toEqual([]);
  });
});
