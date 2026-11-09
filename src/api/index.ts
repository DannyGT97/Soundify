// The ONLY door to the back. Views never call invoke(): they go through `api`,
// so the real implementation and the mock one are swapped in this file alone.
//
// Live progress does not come back from these calls: the back feeds `ui` and
// emits the matching topic (`tasks`, `player`), and the views follow that.

import type {
  AppUpdate, AppUpdateProgress, CopyOutcome, CopyProgress, CreatedPlaylist, DeleteResult, DeviceEntry, DeviceImportScan, DevicesView, DeviceTarget, DownloadNotice, DownloadRequest,
  ExportResult, FormatCatalog, ImportOutcome, ImportScan, PlayerBackend, PlayerMemory, Playlist,
  Preferences, PreferencesPatch,
  StorageInfo, ToolDownload, ToolId, ToolsReport, WindowControls, YtdlpCheck,
} from './types';
import { mockApi } from './mock';
import { insideTauri, tauriApi } from './tauri';
import { SimulatedPlayer } from './mock/player';
import { HtmlPlayer } from './htmlPlayer';
import { AndroidPlayer } from './androidPlayer';
import { onAndroid } from '../platform';

export interface Api {
  /** The library the Gestor, Inicio and the Player show: always the last state known. */
  listPlaylists(): Promise<Playlist[]>;
  formats(): Promise<FormatCatalog>;
  /**
   * Asks YouTube for the playlist and registers it, without downloading.
   * Rejects with an `ApiError`.
   */
  createPlaylist(url: string, format: string, folder: string, numbered: boolean): Promise<CreatedPlaylist>;
  /** Starts a download in the background; only one runs at a time. */
  startDownload(request: DownloadRequest): void;
  stopDownload(): void;
  clearTaskHistory(): void;
  /**
   * Mirrors the back's task and Activity into `ui` from now on (topics `tasks`
   * and `activity`); `onNotice` hears how each download went. Once, at startup.
   */
  watchDownloads(onNotice: (notice: DownloadNotice) => void): void;
  clearActivity(): void;
  /** While the tasks window is open, a result counts as seen the moment it happens. */
  setTasksWindowOpen(open: boolean): void;
  /** The system's folder chooser; null if the user closed it. */
  pickFolder(title: string): Promise<string | null>;
  /** Android: whether the app can use the shared folders, and where the music goes by default. */
  storageInfo(): Promise<StorageInfo>;
  /**
   * Android: opens the system screen where «Acceso a todos los archivos» is
   * granted and resolves when the user is back, with what they decided. Once
   * granted, the library is read again from the folders.
   */
  requestAllFiles(): Promise<boolean>;
  /** Android: the tools in the APK are unpacked (true) or could not be (false). */
  onToolsReady(listener: (ok: boolean) => void): void;
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
  /**
   * «Eliminar»: forgets the playlist+format and, with `files`, deletes what it
   * downloaded first (null when `files` is false). Rejects with an `ApiError`.
   */
  deletePlaylistFormat(blockKey: string, files: boolean): Promise<DeleteResult | null>;
  /** Whether this system can talk to phones, and the ones reachable right now. */
  listDevices(): Promise<DevicesView>;
  /** One folder of a phone (`path` "/"-joined, "" = its root). */
  listDeviceFolder(deviceId: string, path: string): Promise<DeviceEntry[]>;
  /** «Nueva carpeta» of the picker; `path` includes the new name. False if the phone refused. */
  createDeviceFolder(deviceId: string, path: string): Promise<boolean>;
  /** A phone folder in the system's explorer. Rejects with an `ApiError`. */
  openDeviceFolder(deviceId: string, path: string): Promise<void>;
  /** `createPlaylist` with a phone as the destination. Rejects with an `ApiError`. */
  createDevicePlaylist(url: string, format: string, target: DeviceTarget, numbered: boolean): Promise<CreatedPlaylist>;
  /** «Copiar a un dispositivo…»: cancelled with `cancelCopy`, like a folder copy. Rejects with an `ApiError`. */
  copyToDevice(blockKey: string, target: DeviceTarget, onProgress: (progress: CopyProgress) => void): Promise<ExportResult>;
  /** «Copiar a este ordenador…» of a playlist that lives on a phone. Rejects with an `ApiError`. */
  copyFromDevice(blockKey: string, destination: string, register: boolean,
                 onProgress: (progress: CopyProgress) => void): Promise<CopyOutcome>;
  /** The playlists a phone holds under `root` and which are not registered yet. Rejects with an `ApiError`. */
  scanDeviceImport(deviceId: string, deviceName: string, root: string): Promise<DeviceImportScan>;
  /** Registers each phone folder (nothing is copied); one failing never stops the rest. */
  importFromDevice(targets: DeviceTarget[], onProgress: (progress: CopyProgress) => void): Promise<ImportOutcome[]>;
  /**
   * Calls back with the `{0}` of `toolHealth.message` whenever a tool is missing
   * or could not be updated, including what already went wrong before calling.
   */
  watchToolHealth(onProblem: (consequenceKey: string) => void): void;
  /** Calls back with the tools downloading each time that changes (empty = done), starting with the current state. */
  watchToolDownloads(onChange: (downloads: ToolDownload[]) => void): void;
  /** Each tool, whether it is there and its version (asks the tools: not instant). */
  toolsReport(): Promise<ToolsReport>;
  /**
   * Installs a missing tool, or downloads it again with `reinstall` (for
   * yt-dlp, the latest). Progress arrives through `watchToolDownloads`.
   * Rejects with an `ApiError`.
   */
  installTool(id: ToolId, reinstall: boolean): Promise<void>;
  /** Asks GitHub whether there is a newer yt-dlp; downloads nothing. */
  checkYtdlpUpdate(): Promise<YtdlpCheck>;
  /** The tools' folder in the system's file explorer. */
  openToolsFolder(): Promise<void>;
  /** The version of this build, as the build says it. */
  appVersion(): Promise<string>;
  /** Asks GitHub for a newer version; null if this is the newest. Rejects with an `ApiError`. */
  checkAppUpdate(): Promise<AppUpdate | null>;
  /**
   * Downloads, verifies and installs the version `checkAppUpdate` found; the
   * app then closes and the new one opens. Rejects with an `ApiError` and
   * installs NOTHING when the download is not genuine.
   */
  installAppUpdate(onProgress: (progress: AppUpdateProgress) => void): Promise<void>;
  /** The daily check at startup found a newer version. */
  onAppUpdate(listener: (update: AppUpdate) => void): void;
  /** A local file (a path) as a URL the webview can load: covers and the player's files. */
  fileUrl(path: string): string;
  /**
   * The file of a video's thumbnail, or null while it is not on disk: then it
   * is fetched in the background and `onThumbnailReady` gets its id.
   */
  thumbnail(id: string): Promise<string | null>;
  /** The file with the picture of what a URL names (a playlist's own, or one video's); null if there is none. */
  urlThumbnail(url: string, playlist: boolean): Promise<string | null>;
  onThumbnailReady(listener: (id: string) => void): void;
  /** A file of the running download is on disk and in the archive: «Ver archivos» and the Gestor can show it already. */
  onFileDownloaded(listener: () => void): void;
  /** The system's picker for a cover image; null if the user closed it. */
  pickCoverImage(title: string, filter: string): Promise<string | null>;
  /** Makes `image` the cover of the playlist `blockKey` belongs to. Rejects with an `ApiError`. */
  setCover(blockKey: string, image: string): Promise<void>;
  /** The first video of a playlist URL, for «Crear Playlist»'s cover; may ask YouTube. */
  firstVideoId(url: string): Promise<string | null>;
  /** The player's volume and last file, as saved. */
  playerMemory(): Promise<PlayerMemory>;
  savePlayerMemory(memory: PlayerMemory): Promise<void>;
  /** Ajustes, as saved in settings.json. */
  preferences(): Promise<Preferences>;
  /** Saved on each change: there is no «Guardar» button. */
  savePreferences(patch: PreferencesPatch): Promise<void>;
  /** Back to the factory values; what comes back is applied on the spot. */
  resetPreferences(): Promise<Preferences>;
  /** The system's picker for a cookies .txt, opened where the last one was; null if closed. */
  pickCookiesFile(title: string, filter: string): Promise<string | null>;
  fileExists(path: string): Promise<boolean>;
  /** The window in full screen or back (a video in Reproduciendo). */
  setFullscreen(on: boolean): void;
  /** Null where the window has no title bar to draw: the browser and Android. */
  windowControls: WindowControls | null;
  /** Runs `task` (and waits for it) before the window closes. */
  onClosing(task: () => Promise<void>): void;
}

// Each phase of MIGRATION-PLAN.md moves its part to the real one (tauri.ts);
// `npm run dev` in a plain browser (and Playwright) keeps the mock.
export const api: Api = insideTauri() ? tauriApi : mockApi;
// The webview's own <audio>/<video> plays the real files; a plain browser has
// none to open, so it keeps the clock. On the phone the music goes through a
// native service (it goes on with the screen off) and only video through the webview.
function makePlayerBackend(): PlayerBackend {
  if (!insideTauri()) return new SimulatedPlayer();
  const html = new HtmlPlayer(path => api.fileUrl(path));
  return onAndroid ? new AndroidPlayer(html) : html;
}
export const playerBackend: PlayerBackend = makePlayerBackend();

export type * from './types';
