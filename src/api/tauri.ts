// The real API, over Tauri's invoke() and the events the back emits.

import { getVersion } from '@tauri-apps/api/app';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import type { Api } from '.';
import type {
  ActivityLine, AppUpdate, AppUpdateProgress, CopyOutcome, CopyProgress, CreatedPlaylist, DeleteResult, DeviceEntry, DeviceImportScan, DevicesView, DownloadNotice, ExportResult,
  FormatCatalog, ImportOutcome, ImportScan, PlayerMemory, Playlist, Preferences, StorageInfo, TasksViewWire, ToolDownload, ToolsReport, YtdlpCheck,
} from './types';
import { onAndroid } from '../platform';
import { emit, ui } from '../state';
import { applyTasksView } from '../tasks';

export const tauriApi: Api = {
  listPlaylists: () => invoke<Playlist[]>('list_playlists'),
  refreshLibrary: () => invoke('refresh_library'),
  formats: () => invoke<FormatCatalog>('formats'),
  createPlaylist: (url, format, folder, numbered, reuseExisting) =>
    invoke<CreatedPlaylist>('create_playlist', { url, format, folder, numbered, reuseExisting: reuseExisting ?? false }),
  startDownload: request => void invoke('start_download', { request }),
  stopDownload: taskId => void invoke('stop_download', { taskId: typeof taskId === 'number' ? taskId : null }),
  removeQueued: id => void invoke('remove_queued', { id }),
  clearQueue: () => void invoke('clear_queue'),
  clearTaskHistory: () => void invoke('clear_task_history'),
  watchDownloads: onNotice => {
    // Listening first, asking second, as with the tools' health: what happens
    // in between arrives as an event and the snapshot does not lose it.
    void Promise.all([
      listen<TasksViewWire>('tasks', event => applyTasksView(event.payload)),
      listen<ActivityLine>('activity', event => { ui.activity.push(event.payload); emit('activity'); }),
      listen('activity-reset', () => { ui.activity = []; emit('activity'); }),
      listen<DownloadNotice>('notice', event => onNotice(event.payload)),
    ]).then(async () => {
      const [view, lines] = await Promise.all([invoke<TasksViewWire>('tasks_view'), invoke<ActivityLine[]>('activity_log')]);
      applyTasksView(view);
      ui.activity = lines;
      emit('activity');
    });
  },
  clearActivity: () => {
    ui.activity = [];
    emit('activity');
    void invoke('clear_activity');
  },
  setTasksWindowOpen: open => void invoke('set_tasks_window_open', { open }),
  pickFolder: async title => {
    // Android: the dialog plugin hands back a content:// URI; the downloads need a real path.
    if (onAndroid) return invoke<string | null>('pick_phone_folder');
    const chosen = await open({ directory: true, multiple: false, title });
    return typeof chosen === 'string' ? chosen : null;
  },
  storageInfo: () => invoke<StorageInfo>('storage_info'),
  requestAllFiles: () => invoke<boolean>('request_all_files'),
  onToolsReady: listener => void listen<boolean>('tools-ready', event => listener(event.payload)),
  copyPlaylist: (blockKey, destination, register, onProgress) =>
    withCopyProgress(onProgress, () => invoke<CopyOutcome>('copy_playlist', { blockKey, destination, register })),
  cancelCopy: () => void invoke('cancel_copy'),
  scanImport: folder => invoke<ImportScan>('scan_import', { folder }),
  importPlaylists: folders => invoke<ImportOutcome[]>('import_playlists', { folders }),
  revealFolder: folder => invoke('open_folder', { folder }),
  deletePlaylistFormat: (blockKey, files) => invoke<DeleteResult | null>('delete_playlist_format', { blockKey, files }),
  listDevices: () => invoke<DevicesView>('list_devices'),
  listDeviceFolder: (deviceId, path) => invoke<DeviceEntry[]>('list_device_folder', { deviceId, path }),
  createDeviceFolder: (deviceId, path) => invoke<boolean>('create_device_folder', { deviceId, path }),
  openDeviceFolder: (deviceId, path) => invoke('open_device_folder', { deviceId, path }),
  createDevicePlaylist: (url, format, target, numbered, reuseExisting) =>
    invoke<CreatedPlaylist>('create_device_playlist', { url, format, target, numbered, reuseExisting: reuseExisting ?? false }),
  copyToDevice: (blockKey, target, onProgress) =>
    withCopyProgress(onProgress, () => invoke<ExportResult>('copy_to_device', { blockKey, target })),
  copyFromDevice: (blockKey, destination, register, onProgress) =>
    withCopyProgress(onProgress, () => invoke<CopyOutcome>('copy_from_device', { blockKey, destination, register })),
  scanDeviceImport: (deviceId, deviceName, root) => invoke<DeviceImportScan>('scan_device_import', { deviceId, deviceName, root }),
  importFromDevice: (targets, onProgress) =>
    withCopyProgress(onProgress, () => invoke<ImportOutcome[]>('import_from_device', { targets })),
  watchToolHealth: onProblem => {
    // Listening first, asking second: a problem noted down in between would
    // otherwise be lost.
    void listen<string>('tool-health', event => onProblem(event.payload)).then(async () => {
      const key = await invoke<string | null>('tool_health');
      if (key) onProblem(key);
    });
  },
  watchToolDownloads: onChange => {
    void listen<ToolDownload[]>('tool-downloads', event => onChange(event.payload)).then(async () => {
      onChange(await invoke<ToolDownload[]>('tool_downloads'));
    });
  },
  toolsReport: () => invoke<ToolsReport>('tools_report'),
  installTool: (id, reinstall) => invoke('install_tool', { id, reinstall }),
  checkYtdlpUpdate: () => invoke<YtdlpCheck>('check_ytdlp_update'),
  openToolsFolder: () => invoke('open_tools_folder'),
  appVersion: () => getVersion(),
  checkAppUpdate: () => invoke<AppUpdate | null>('check_app_update'),
  installAppUpdate: async onProgress => {
    const stop = await listen<AppUpdateProgress>('app-update-progress', event => onProgress(event.payload));
    try {
      await invoke('install_app_update');
    } finally {
      stop();
    }
  },
  onAppUpdate: listener => void listen<AppUpdate>('app-update', event => listener(event.payload)),
  fileUrl: path => convertFileSrc(path),
  thumbnail: id => invoke<string | null>('thumbnail', { id }),
  urlThumbnail: (url, playlist) => invoke<string | null>('url_thumbnail', { url, playlist }),
  onThumbnailReady: listener => void listen<string>('thumbnail-ready', event => listener(event.payload)),
  onFileDownloaded: listener => void listen('file-downloaded', () => listener()),
  pickCoverImage: (title, filter) => invoke<string | null>('pick_cover_image', { title, filter }),
  setCover: (blockKey, image) => invoke('set_cover', { blockKey, image }),
  firstVideoId: url => invoke<string | null>('first_video_id', { url }),
  playerMemory: () => invoke<PlayerMemory>('player_memory'),
  savePlayerMemory: memory => invoke('save_player_memory', { memory }),
  preferences: () => invoke<Preferences>('preferences'),
  savePreferences: patch => invoke('save_preferences', { patch }),
  resetPreferences: () => invoke<Preferences>('reset_preferences'),
  pickCookiesFile: (title, filter) => invoke<string | null>('pick_cookies_file', { title, filter }),
  fileExists: path => invoke<boolean>('file_exists', { path }),
  playbackDevice: async () => {
    if (!onAndroid) return { kind: 'this', name: null };
    const answer = await invoke<{ kind?: string; name?: string }>('native_player', { action: 'playerOutputDevice' }).catch(() => null);
    const kind = answer?.kind === 'bluetooth' || answer?.kind === 'headphones' ? answer.kind : 'this';
    return { kind, name: answer?.name || null };
  },
  // Android may refuse: the screen still covers the app, under the system's bars.
  setFullscreen: on => void getCurrentWindow().setFullscreen(on).catch(() => {}),
  windowControls: onAndroid ? null : {
    minimize: () => void getCurrentWindow().minimize(),
    toggleMaximize: () => void getCurrentWindow().toggleMaximize(),
    close: () => void getCurrentWindow().close(),
    isMaximized: () => getCurrentWindow().isMaximized(),
    onResized: listener => void getCurrentWindow().onResized(() => listener()),
    showSystemMenu: () => void invoke('show_system_menu'),
  },
  onClosing: task => {
    // Not prevented: the handler is awaited before the window goes away.
    void getCurrentWindow().onCloseRequested(async () => { await task(); });
  },
};

/** Hears `copy-progress` only while `call` runs: every copy reports on that one topic. */
async function withCopyProgress<T>(onProgress: (progress: CopyProgress) => void, call: () => Promise<T>): Promise<T> {
  const stop = await listen<CopyProgress>('copy-progress', event => onProgress(event.payload));
  try {
    return await call();
  } finally {
    stop();
  }
}

/** Inside the Tauri window, not in a plain browser running `npm run dev`. */
export function insideTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
