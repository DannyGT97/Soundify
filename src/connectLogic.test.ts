import { describe, expect, it } from 'vitest';
import { actionsFor, projectPosition, shouldPublish, toWireState, type PlayerView } from './connectLogic';
import type { ConnectPlayerState, Track } from './api';

const track: Track = { id: 'abc', number: 1, title: 'Canción', addedDaysAgo: 0, seconds: 200, status: 'ok', file: '/m/a.mp3' };
const view: PlayerView = { nowPlaying: { blockKey: 'b1', track }, playing: true, position: 12, duration: 200, volume: 0.5, shuffle: false, repeat: 'off' };
const blockOf = () => ({ playlist: 'Mix', format: 'MP3', video: false });

function state(patch: Partial<ConnectPlayerState> = {}): ConnectPlayerState {
  return { nowPlaying: { videoId: 'abc', title: 'Canción', playlist: 'Mix', format: 'MP3', video: false }, playing: true, position: 10, duration: 200, volume: 0.5, shuffle: false, repeat: 'off', ...patch };
}

describe('toWireState', () => {
  it('says the file by its video id, with the playlist and format of its block', () => {
    expect(toWireState(view, blockOf)).toEqual(state({ position: 12 }));
  });

  it('says nothing is playing when nothing is, and keeps the numbers in range', () => {
    const wire = toWireState({ ...view, nowPlaying: null, position: -3, volume: 4 }, blockOf);
    expect(wire.nowPlaying).toBeNull();
    expect(wire.position).toBe(0);
    expect(wire.volume).toBe(1);
  });

  it('does not fail when the block is not in the library any more', () => {
    expect(toWireState(view, () => null).nowPlaying).toMatchObject({ videoId: 'abc', playlist: '', format: '', video: false });
  });
});

describe('shouldPublish', () => {
  it('publishes the first state', () => {
    expect(shouldPublish(null, state(), 0)).toBe(true);
  });

  it('publishes any change but the position at once', () => {
    const last = state();
    for (const patch of [{ playing: false }, { volume: 0.2 }, { shuffle: true }, { repeat: 'all' as const }, { duration: 180 }, { nowPlaying: null }]) {
      expect(shouldPublish(last, state(patch), 0.1)).toBe(true);
    }
  });

  it('does not repeat itself while the position just goes on', () => {
    expect(shouldPublish(state({ position: 10 }), state({ position: 12 }), 2)).toBe(false);
    expect(shouldPublish(state({ position: 10 }), state({ position: 10.4 }), 0.4)).toBe(false);
  });

  it('publishes a jump of the position, either way, and a still position that is not still', () => {
    expect(shouldPublish(state({ position: 10 }), state({ position: 90 }), 1)).toBe(true);
    expect(shouldPublish(state({ position: 90 }), state({ position: 5 }), 1)).toBe(true);
    // Paused: it was not going anywhere, so a different position is a seek.
    expect(shouldPublish(state({ playing: false, position: 10 }), state({ playing: false, position: 20 }), 5)).toBe(true);
    expect(shouldPublish(state({ playing: false, position: 10 }), state({ playing: false, position: 10 }), 5)).toBe(false);
  });
});

describe('projectPosition', () => {
  it('moves on while it plays and stays while it is paused, never past the end', () => {
    expect(projectPosition(state({ position: 10 }), 3)).toBe(13);
    expect(projectPosition(state({ playing: false, position: 10 }), 3)).toBe(10);
    expect(projectPosition(state({ position: 199 }), 5)).toBe(200);
    expect(projectPosition(state({ position: 10 }), -1)).toBe(10);
  });
});

describe('actionsFor', () => {
  const now = { playing: true, shuffle: false, repeat: 'off' as const };

  it('presses play or pause only when it changes something', () => {
    expect(actionsFor({ cmd: 'play' }, now)).toEqual([]);
    expect(actionsFor({ cmd: 'pause' }, now)).toEqual([{ kind: 'toggle' }]);
    expect(actionsFor({ cmd: 'play' }, { ...now, playing: false })).toEqual([{ kind: 'toggle' }]);
    expect(actionsFor({ cmd: 'toggle' }, now)).toEqual([{ kind: 'toggle' }]);
  });

  it('shuffle on / off only flips when it differs, and with no value it flips', () => {
    expect(actionsFor({ cmd: 'shuffle', on: true }, now)).toEqual([{ kind: 'shuffle' }]);
    expect(actionsFor({ cmd: 'shuffle', on: false }, now)).toEqual([]);
    expect(actionsFor({ cmd: 'shuffle' }, { ...now, shuffle: true })).toEqual([{ kind: 'shuffle' }]);
  });

  it('repeat goes round the modes the shortest number of presses', () => {
    expect(actionsFor({ cmd: 'repeat', mode: 'one' }, now)).toEqual([{ kind: 'repeat', times: 2 }]);
    expect(actionsFor({ cmd: 'repeat', mode: 'off' }, { ...now, repeat: 'one' })).toEqual([{ kind: 'repeat', times: 1 }]);
    expect(actionsFor({ cmd: 'repeat', mode: 'off' }, now)).toEqual([]);
    expect(actionsFor({ cmd: 'repeat' }, now)).toEqual([{ kind: 'repeat', times: 1 }]);
  });

  it('a load names the file at the index of the queue', () => {
    const queue = [{ videoId: 'a', title: 'A' }, { videoId: 'b', title: 'B' }];
    expect(actionsFor({ cmd: 'load', playlist: 'Mix', queue, index: 1, position: 30, autoplay: true }, now)).toEqual([
      { kind: 'load', playlist: 'Mix', videoId: 'b', position: 30, autoplay: true },
    ]);
    expect(actionsFor({ cmd: 'load', playlist: 'Mix', queue, index: 9, position: 0, autoplay: true }, now)).toEqual([]);
  });
});
