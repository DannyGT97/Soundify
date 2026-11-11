// UI state shared by every screen. The preferences come from settings.json
// (see `preferences.ts`); the task and Activity data are the back's, mirrored
// here (see `tasks.ts`).

import type { ActivityLine, TaskOrigin, Track } from './api/types';
import type { Repeat } from './playerQueue';

export type { Repeat } from './playerQueue';
export type Theme = 'dark' | 'light' | 'dark-neutral' | 'light-neutral' | 'dark-red' | 'dark-blue' | 'amoled' | 'light-red' | 'light-blue';
export type PlayerPlacement = 'side' | 'bottom' | 'both';

export interface NowPlaying { blockKey: string; track: Track; }
export type TaskStatus = 'ok' | 'warning' | 'stopped' | 'failed';
/** `fraction` is 0–1 while downloading, null while it cannot tell (preparing). */
/** `blockKey`: the playlist+format block being downloaded, once the back knows it (null before, and for a loose URL's own archive). */
/** A download ordered while another one runs: it starts by itself when its turn comes. */
export interface QueuedTask { id: number; title: string; origin: TaskOrigin; blockKey: string | null; }
export interface RunningTask { id: number; title: string; origin: TaskOrigin; step: 'prepare' | 'download' | 'retry'; fraction: number | null; total: number; blockKey: string | null; }
/** `summary` is an i18n key (with `summaryArgs`), or text the Java app already translated. */
export interface FinishedTask { title: string; origin: TaskOrigin; status: TaskStatus; summary: string | null; summaryArgs: string[]; finishedAt: string; }

export const ui = {
  sidebarCollapsed: false,
  theme: 'dark' as Theme,
  playerPlacement: 'both' as PlayerPlacement,
  lastPlayerPage: 'player' as 'player' | 'now-playing',
  expandedPlaylists: new Set<string>(['pl1']),
  settingsTab: 'general',
  nowPlaying: null as NowPlaying | null,
  playing: false,
  position: 0,
  /** Of the file loaded, once the engine has opened it (the archive does not store it). */
  duration: 0,
  /** The video is in the floating panel (`components/pip.ts`), not on any screen. */
  pip: false,
  /** The panel opened by itself on leaving Reproduciendo (so it goes back when that is opened again). */
  pipAuto: false,
  /** A video goes to the floating panel by itself on leaving Reproduciendo (a preference). */
  autoPip: true,
  shuffle: false,
  repeat: 'off' as Repeat,
  volume: 0.7,
  /** The first task running (what the pill and Tareas lead with); null when nothing runs. */
  task: null as RunningTask | null,
  /** Everything running now: one, or two with Ajustes → «Descargar más rápido» (experimental). */
  running: [] as RunningTask[],
  queue: [] as QueuedTask[],
  unseenResult: null as TaskStatus | null,
  history: [] as FinishedTask[],
  activity: [] as ActivityLine[],
};

type Topic = 'library' | 'player' | 'tasks' | 'activity' | 'theme' | 'player-placement' | 'tools' | 'storage';
const listeners = new Map<Topic, Set<() => void>>();

export function subscribe(topic: Topic, listener: () => void): () => void {
  const set = listeners.get(topic) ?? new Set();
  set.add(listener);
  listeners.set(topic, set);
  return () => set.delete(listener);
}
export function emit(topic: Topic): void { listeners.get(topic)?.forEach(listener => listener()); }
/**
 * `subscribe` for a piece of the shell, which is drawn again on a language
 * change: once `node` has been on the page and left it, it stops listening.
 */
export function subscribeWhile(node: Node, topic: Topic, listener: () => void): void {
  let seen = false;
  const stop = subscribe(topic, () => {
    if (node.isConnected) {
      seen = true;
      listener();
    } else if (seen) {
      stop();
    }
  });
}
export const THEMES: readonly Theme[] = ['dark', 'light', 'dark-neutral', 'light-neutral', 'dark-red', 'dark-blue', 'amoled', 'light-red', 'light-blue'];
export const PLAYER_PLACEMENTS: readonly PlayerPlacement[] = ['side', 'bottom', 'both'];
/** Applies it on screen; saving it is `preferences.ts`'s. */
export function setTheme(theme: Theme): void {
  ui.theme = theme;
  document.documentElement.dataset.theme = theme;
  emit('theme');
}
export function setPlayerPlacement(value: PlayerPlacement): void {
  ui.playerPlacement = value;
  emit('player-placement');
}
