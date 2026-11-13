// The pure part of Soundify Connect on the page: what this player says about itself, when it is
// worth saying again, and how a state that arrived a moment ago is brought up to now. The
// stateful part (listening, routing the controls) is `connect.ts`.

import type { ConnectCommand, ConnectPlayerState, Track } from './api';

/** What `ui` knows of the player, as far as Connect needs. */
export interface PlayerView {
  nowPlaying: { blockKey: string; track: Track } | null;
  playing: boolean;
  position: number;
  duration: number;
  volume: number;
  shuffle: boolean;
  repeat: 'off' | 'all' | 'one';
}

/** Where a block's playlist and format are, so the state can say them (the page owns the library). */
export interface BlockInfo { playlist: string; format: string; video: boolean }

/** This player's state as the protocol says it. */
export function toWireState(view: PlayerView, blockOf: (blockKey: string) => BlockInfo | null): ConnectPlayerState {
  const now = view.nowPlaying;
  const block = now ? blockOf(now.blockKey) : null;
  return {
    nowPlaying: now ? { videoId: now.track.id, title: now.track.title, playlist: block?.playlist ?? '', format: block?.format ?? '', video: block?.video ?? false } : null,
    playing: view.playing,
    position: Math.max(0, view.position),
    duration: Math.max(0, view.duration),
    volume: Math.min(1, Math.max(0, view.volume)),
    shuffle: view.shuffle,
    repeat: view.repeat,
  };
}

/** The position moves on its own while it plays: it is not worth saying again unless it jumped. */
const DRIFT_SECONDS = 2.5;

/**
 * Is `next` worth publishing after `last` (published `elapsed` seconds ago)? Anything but the
 * position changing says so at once; the position only when it is not where playing on from `last` would put it.
 */
export function shouldPublish(last: ConnectPlayerState | null, next: ConnectPlayerState, elapsed: number): boolean {
  if (!last) return true;
  if (last.nowPlaying?.videoId !== next.nowPlaying?.videoId || last.playing !== next.playing) return true;
  if (last.volume !== next.volume || last.shuffle !== next.shuffle || last.repeat !== next.repeat || last.duration !== next.duration) return true;
  return Math.abs(projectPosition(last, elapsed) - next.position) > DRIFT_SECONDS;
}

/** Where a state that arrived `elapsed` seconds ago is now. */
export function projectPosition(state: ConnectPlayerState, elapsed: number): number {
  const moved = state.playing ? state.position + Math.max(0, elapsed) : state.position;
  return state.duration > 0 ? Math.min(moved, state.duration) : moved;
}

/** A stand-in for the track of another device's file: the page only needs its id and title. */
export function remoteTrack(videoId: string, title: string, duration: number): Track {
  return { id: videoId, number: 0, title, addedDaysAgo: 0, seconds: duration, status: 'ok', file: null };
}

/**
 * What an incoming command does, as calls on the local player. `now` is the current state, so
 * «turn on» and «go to this mode» only press the button when it is needed.
 */
export type LocalAction =
  | { kind: 'toggle' } | { kind: 'next' } | { kind: 'previous' }
  | { kind: 'seek'; seconds: number } | { kind: 'volume'; volume: number }
  | { kind: 'shuffle' } | { kind: 'repeat'; times: number }
  | { kind: 'load'; playlist: string; videoId: string; position: number; autoplay: boolean };

export function actionsFor(command: ConnectCommand, now: { playing: boolean; shuffle: boolean; repeat: 'off' | 'all' | 'one' }): LocalAction[] {
  switch (command.cmd) {
    case 'play': return now.playing ? [] : [{ kind: 'toggle' }];
    case 'pause': return now.playing ? [{ kind: 'toggle' }] : [];
    case 'toggle': return [{ kind: 'toggle' }];
    case 'next': return [{ kind: 'next' }];
    case 'previous': return [{ kind: 'previous' }];
    case 'seek': return [{ kind: 'seek', seconds: command.seconds }];
    case 'volume': return [{ kind: 'volume', volume: command.volume }];
    case 'shuffle': return command.on === undefined || command.on !== now.shuffle ? [{ kind: 'shuffle' }] : [];
    case 'repeat': {
      if (command.mode === undefined) return [{ kind: 'repeat', times: 1 }];
      const order = ['off', 'all', 'one'] as const;
      const times = (order.indexOf(command.mode) - order.indexOf(now.repeat) + 3) % 3;
      return times > 0 ? [{ kind: 'repeat', times }] : [];
    }
    case 'load': {
      const item = command.queue[command.index];
      return item ? [{ kind: 'load', playlist: command.playlist, videoId: item.videoId, position: command.position, autoplay: command.autoplay }] : [];
    }
  }
}
