// The ONLY door to the back. Views never call invoke(): they go through `api`,
// so the real implementation and the mock one are swapped in this file alone.
//
// Live progress does not come back from these calls: the back feeds `ui` and
// emits the matching topic (`tasks`, `player`), and the views follow that.

import type {
  CopyOutcome, CopyProgress, DownloadRequest, FormatCatalog, ImportOutcome, ImportScan, PlayerBackend, Playlist,
} from './types';
import { mockApi } from './mock';
import { insideTauri, tauriApi } from './tauri';
import { SimulatedPlayer } from './mock/player';

export interface Api {
  /** The library the Gestor, Inicio and the Player show: always the last state known. */
  listPlaylists(): Promise<Playlist[]>;
  formats(): Promise<FormatCatalog>;
  /** Starts a download in the background; only one runs at a time. */
  startDownload(request: DownloadRequest): void;
  stopDownload(): void;
  clearTaskHistory(): void;
  /** The raw yt-dlp output of the download in progress. */
  activityLog(): Promise<string[]>;
  /** The system's folder chooser; null if the user closed it. */
  pickFolder(title: string): Promise<string | null>;
  /** Copies a playlist+format into its own folder inside `destination`. Rejects with an `ApiError`. */
  copyPlaylist(blockKey: string, destination: string, register: boolean,
               onProgress: (progress: CopyProgress) => void): Promise<CopyOutcome>;
  /** Stops the copy between one file and the next; what it had copied is undone. */
  cancelCopy(): void;
  /** The playlists under a folder (up to two levels down) and which are not registered yet. */
  scanImport(folder: string): Promise<ImportScan>;
  /** Registers each folder; one failing never stops the rest. */
  importPlaylists(folders: string[]): Promise<ImportOutcome[]>;
  /** Shows a folder in the system's file explorer. */
  revealFolder(folder: string): void;
}

// Each phase of MIGRATION-PLAN.md moves its part to the real one (tauri.ts);
// `npm run dev` in a plain browser (and Playwright) keeps the mock.
export const api: Api = insideTauri() ? tauriApi : mockApi;
export const playerBackend: PlayerBackend = new SimulatedPlayer();

export type * from './types';
