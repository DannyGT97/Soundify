// What the front and the back say to each other. The shapes follow the Java
// app's archive (ArchiveStore) as the views use it.

export type Status = 'ok' | 'pending' | 'failed' | 'unavailable';

export interface Track {
  id: string;
  number: number;
  title: string;
  addedDaysAgo: number;
  /** 0 until the player opens the file: the archive does not store durations. */
  seconds: number;
  status: Status;
  /** Why it is unavailable, as an i18n key (`unavailable.*`): the back recognises it, the front words it. */
  reason?: string | null;
  /** The file to play, when it is on disk (a path or a content:// URI). */
  file?: string | null;
}

/** One playlist in one format: what the Gestor shows as a block and the Player as a row. */
export interface FormatBlock {
  key: string;
  playlistId: string;
  format: string;
  video: boolean;
  numbered: boolean;
  sync: boolean;
  lastSync: string;
  tracks: Track[];
}

export interface Playlist {
  id: string;
  title: string;
  /** A path on desktop, a content:// URI on Android: shown, never parsed. */
  folder: string;
  formats: FormatBlock[];
}

/** The formats yt-dlp says it supports, already split by kind. */
export interface FormatCatalog {
  audio: string[];
  video: string[];
}

/** A failed command: an i18n key and its arguments, so it follows the language in use. */
export interface ApiError {
  key: string;
  args: string[];
}

export interface CopyProgress {
  done: number;
  total: number;
  file: string;
}

export interface ExportResult {
  copied: number;
  skipped: number;
  missing: number;
  cancelled: boolean;
  /** Files copied in this run and deleted again because it was cancelled. */
  undone: number;
}

export type RegisterOutcome = 'notRequested' | 'registered' | 'alreadyRegistered' | 'failed';

export interface CopyOutcome {
  result: ExportResult;
  /** The folder the copy left: it can be imported as is somewhere else. */
  folder: string;
  register: RegisterOutcome;
}

export interface ImportCandidate {
  folder: string;
  title: string;
  format: string;
}

/** Every playlist found under the folder (`found`) and the ones not registered yet. */
export interface ImportScan {
  found: number;
  candidates: ImportCandidate[];
}

export interface ImportResult {
  title: string;
  format: string;
  total: number;
  downloaded: number;
  pending: number;
}

export interface ImportOutcome {
  folder: string;
  result: ImportResult | null;
  error: ApiError | null;
  alreadyRegistered: boolean;
}

export type TaskOrigin = 'url' | 'manager';

/** What the user asked to download, from the screen that asked for it. */
export type DownloadRequest =
  | { kind: 'single'; url: string; format: string; folder: string }
  | { kind: 'new-playlist'; url: string; format: string; folder: string; numbered: boolean }
  | { kind: 'update'; blockKey: string };

/**
 * The engine that makes a file sound. On desktop it will be the webview's own
 * <audio>/<video>; on Android a native service, so music goes on with the
 * screen off. The queue, shuffle and repeat stay in `player.ts`, above it.
 */
export interface PlayerBackend {
  /** Loads a file and leaves it at second 0, paused. */
  load(blockKey: string, track: Track): void;
  play(): void;
  pause(): void;
  seek(seconds: number): void;
  setVolume(volume: number): void;
  unload(): void;
  onTime(listener: (seconds: number) => void): void;
  onEnded(listener: () => void): void;
}
