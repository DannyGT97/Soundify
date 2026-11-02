// UI state shared by every screen. Persistent preferences are stored locally;
// playback and task data stay as session-only simulations.

import type { TaskOrigin, Track } from './api/types';

export type Repeat = 'off' | 'all' | 'one';
export type Theme = 'dark' | 'light' | 'dark-red' | 'dark-blue' | 'amoled' | 'light-red' | 'light-blue';
export type PlayerPlacement = 'side' | 'bottom' | 'both';

export interface NowPlaying { blockKey: string; track: Track; }
export type TaskStatus = 'ok' | 'stopped';
export interface RunningTask { title: string; origin: TaskOrigin; step: 'prepare' | 'download' | 'retry'; done: number; total: number; }
export interface FinishedTask { title: string; origin: TaskOrigin; status: TaskStatus; finishedAt: string; }

function stored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return allowed.includes(value as T) ? value as T : fallback;
  } catch {
    // Private mode or no storage at all (unit tests): the default, just not remembered.
    return fallback;
  }
}

export const ui = {
  sidebarCollapsed: false,
  summaryHidden: true,
  theme: stored('soundify.theme', ['dark', 'light', 'dark-red', 'dark-blue', 'amoled', 'light-red', 'light-blue'] as const, 'dark'),
  playerPlacement: stored('soundify.playerPlacement', ['side', 'bottom', 'both'] as const, 'both'),
  lastPlayerPage: 'player' as 'player' | 'now-playing',
  selectedBlock: null as string | null,
  expandedPlaylists: new Set<string>(['pl1']),
  settingsTab: 'general',
  nowPlaying: null as NowPlaying | null,
  playing: false,
  position: 0,
  shuffle: false,
  repeat: 'off' as Repeat,
  volume: 0.7,
  task: null as RunningTask | null,
  unseenResult: null as TaskStatus | null,
  history: [] as FinishedTask[],
};

type Topic = 'library' | 'player' | 'tasks' | 'theme' | 'player-placement';
const listeners = new Map<Topic, Set<() => void>>();

export function subscribe(topic: Topic, listener: () => void): () => void {
  const set = listeners.get(topic) ?? new Set();
  set.add(listener);
  listeners.set(topic, set);
  return () => set.delete(listener);
}
export function emit(topic: Topic): void { listeners.get(topic)?.forEach(listener => listener()); }
export function setTheme(theme: Theme): void {
  ui.theme = theme;
  localStorage.setItem('soundify.theme', theme);
  document.documentElement.dataset.theme = theme;
  emit('theme');
}
export function setPlayerPlacement(value: PlayerPlacement): void {
  ui.playerPlacement = value;
  localStorage.setItem('soundify.playerPlacement', value);
  emit('player-placement');
}
