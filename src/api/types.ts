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
  /** The video whose thumbnail is the default cover: the first in the playlist's order. */
  firstVideoId?: string | null;
  /** Whether its folder (or phone) can be reached now; «Eliminar todo» is offered only then. Absent = yes. */
  accessible?: boolean;
  /** Set when its files live on a phone: copy, open and delete go through the device. */
  device?: DeviceTarget | null;
  tracks: Track[];
}

/** A phone and a folder on it (`path` is "/"-joined, "" = its root). */
export interface DeviceTarget {
  deviceId: string;
  deviceName: string;
  path: string;
}

export interface DeviceInfo {
  id: string;
  name: string;
}

export interface DeviceEntry {
  name: string;
  isFolder: boolean;
  sizeBytes: number;
}

/** Whether this system can talk to phones at all, and the ones connected now. */
export interface DevicesView {
  available: boolean;
  devices: DeviceInfo[];
}

/** An import candidate found on a phone: `folder` is the folder's own name. */
export interface DeviceImportCandidate extends ImportCandidate {
  target: DeviceTarget;
}

export interface DeviceImportScan {
  found: number;
  candidates: DeviceImportCandidate[];
}

/** What «Eliminar todo» did; `total` (deleted + unregistered) is what the user is told. */
export interface DeleteResult {
  deleted: number;
  unregistered: number;
  leftOver: number;
}

export interface Playlist {
  id: string;
  title: string;
  /** A path on desktop, a content:// URI on Android: shown, never parsed. */
  folder: string;
  /** The picture the user picked as its cover; otherwise its first video's thumbnail is shown. */
  cover?: string | null;
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
  | { kind: 'single-to-device'; url: string; format: string; target: DeviceTarget }
  | { kind: 'update'; blockKey: string };

/** A playlist «Crear Playlist» just registered (`blockKey` is what an update goes by). */
export interface CreatedPlaylist {
  blockKey: string;
  title: string;
  count: number;
  /** The real title could not be had and the generic one was used. */
  genericTitle: boolean;
  /** Its numbering, and whether the checkbox decided it or an existing registration did. */
  numberMode: { numberFiles: boolean; source: 'requested' | 'sameFormat' | 'otherFormat' };
}

/** One line of Activity: yt-dlp's output as it came, or an app message as an i18n key. */
export type ActivityLine =
  | { kind: 'raw'; text: string }
  | { kind: 'message'; key: string; args: string[] };

/** How a download went, worth a notice (statuses as `task_history` writes them). */
export interface DownloadNotice { status: string; key: string; args: string[]; }

/** The back's `TasksView`, as it arrives: the front maps it into `ui` (see `tasks.ts`). */
export interface TasksViewWire {
  running: { title: string; origin: string; step: 'prepare' | 'download' | 'retry'; fraction: number | null; total: number; blockKey?: string | null } | null;
  unseen: string | null;
  history: TaskRecordWire[];
}

/** One line of `data/history/tasks.json`; written by the Java app too, so anything may be missing. */
export interface TaskRecordWire {
  status?: string | null;
  title?: string | null;
  origin?: string | null;
  summary?: string | null;
  summaryArgs?: string[];
  finishedAt?: string | null;
}

/**
 * The engine that makes a file sound. On desktop it will be the webview's own
 * <audio>/<video>; on Android a native service, so music goes on with the
 * screen off. The queue, shuffle and repeat stay in `player.ts`, above it.
 */
export interface PlayerBackend {
  /** Loads a file and leaves it at second 0, paused; `onDuration` says when it is ready. */
  load(blockKey: string, track: Track): void;
  play(): void;
  pause(): void;
  seek(seconds: number): void;
  setVolume(volume: number): void;
  unload(): void;
  onTime(listener: (seconds: number) => void): void;
  onEnded(listener: () => void): void;
  /** The file opened and this is how long it lasts: the moment a seek is honoured. */
  onDuration(listener: (seconds: number) => void): void;
  /** The file could not be opened or played. */
  onError(listener: () => void): void;
  /** Where a video is drawn, for Now Playing to show it; null when the engine draws nothing. */
  view(): HTMLVideoElement | null;
  /** Takes the view back out of wherever it was shown, without stopping it. */
  park(): void;
}

/** What the player keeps between runs (the Java `UserSettings.playerVolume` and `lastPlayed*`). */
export interface PlayerMemory { volume: number; last: LastPlayed | null }
/** What Ajustes shows and changes, from settings.json. `appearance` is the theme id. */
export interface Preferences {
  appearance: string;
  language: string;
  uiScale: number;
  playerPlacement: string;
  downloadThumbnails: boolean;
  includeIdInTitleName: boolean;
  appUpdateCheckEnabled: boolean;
  /** Ajustes → Herramientas: bring yt-dlp up to date at startup. */
  autoUpdates: boolean;
  cookiesFile: string;
  lastAppUpdateCheck: string;
}
/** A change from Ajustes: only what it carries is touched. */
export type PreferencesPatch = Partial<Omit<Preferences, 'lastAppUpdateCheck'>>;
/** The last file played, to bring it back paused at startup. `archive` is its block key. */
export interface LastPlayed { archive: string; trackId: string; playlist: string; format: string; position: number }

/** A tool being downloaded; `percent` is null when the server does not say the size. */
export interface ToolDownload { id: ToolId; tool: string; percent: number | null }

export type ToolId = 'ytdlp' | 'ffmpeg' | 'deno';
/** One tool as Ajustes → Herramientas shows it. */
export interface ToolStatus {
  id: ToolId;
  name: string;
  installed: boolean;
  /** What the tool says of itself; null if it is missing or does not answer. */
  version: string | null;
  /** Missing, but a copy travels with the app: installing it is a copy, not a download. */
  bundled: boolean;
  /** Without it nothing downloads (Deno is only recommended). */
  required: boolean;
}
export interface ToolsReport {
  /** The app keeps its own copies (Windows); elsewhere the system's are used and nothing is installed from here. */
  managed: boolean;
  folder: string;
  tools: ToolStatus[];
}
/** «Buscar actualización» of yt-dlp; `latest` is null when GitHub did not answer. */
export interface YtdlpCheck { latest: string | null; newer: boolean }

/** A newer version of the app, with what it brings (already plain text: it comes from outside). */
export interface AppUpdate {
  version: string;
  notes: string;
}

/** How the installation is going: the download, then checked, then handed over. */
export type AppUpdateProgress =
  | { phase: 'downloading'; version: string; percent: number }
  | { phase: 'verified' }
  | { phase: 'installing'; version: string };
