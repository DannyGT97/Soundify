// PLACEHOLDER API: the library comes from data.ts and a download only advances
// a counter, so the menu's indicator and the tasks window can be seen in every
// state. The real one is fed by the yt-dlp process (its --print hooks and
// progress lines) and keeps the same contract: it fills `ui` and emits.

import type { Api } from '..';
import type {
  CopyOutcome, CopyProgress, CreatedPlaylist, DeleteResult, DeviceEntry, DeviceTarget, DownloadNotice, DownloadRequest, ExportResult, PlayerMemory, Preferences, TaskOrigin,
} from '../types';
import { emit, ui, type TaskStatus } from '../../state';
import { activityLog, AUDIO_FORMATS, importedPlaylist, playlists, VIDEO_FORMATS } from './data';
import { mockTools } from './tools';

let timer: number | undefined;

export const mockApi: Api = {
  listPlaylists: async () => playlists,
  formats: async () => ({ audio: AUDIO_FORMATS, video: VIDEO_FORMATS }),
  createPlaylist,
  startDownload,
  stopDownload: () => { if (ui.task) finish('stopped'); },
  clearTaskHistory: () => {
    ui.history = [];
    emit('tasks');
  },
  watchDownloads: listener => {
    onNotice = listener;
    ui.activity = activityLog.map(text => ({ kind: 'raw', text }));
    emit('activity');
  },
  clearActivity: () => {
    ui.activity = [];
    emit('activity');
  },
  setTasksWindowOpen: () => {},
  // There is no folder chooser in a plain browser: always the same made-up one.
  pickFolder: async () => 'D:\\Música\\Descargas',
  copyPlaylist,
  cancelCopy: () => { copyCancelled = true; },
  scanImport: async folder => ({
    found: 2,
    candidates: [{ folder: `${folder}\\Lo-fi para estudiar`, title: 'Lo-fi para estudiar', format: 'mp3' }],
  }),
  importPlaylists: async folders => folders.map(folder => {
    const playlist = importedPlaylist();
    return {
      folder,
      result: { title: playlist?.title ?? 'Lo-fi para estudiar', format: 'mp3', total: 12, downloaded: 10, pending: 2 },
      error: null,
      alreadyRegistered: false,
    };
  }),
  revealFolder: () => {},
  deletePlaylistFormat,
  // One made-up phone, always connected, with a playlist already on it to import.
  listDevices: async () => ({ available: true, devices: [{ id: MOCK_PHONE.deviceId, name: MOCK_PHONE.deviceName }] }),
  listDeviceFolder: async (_deviceId, path) => {
    await new Promise(resolve => window.setTimeout(resolve, 200));
    return (PHONE_FOLDERS[path] ?? []).map((name): DeviceEntry => ({ name, isFolder: true, sizeBytes: 0 }));
  },
  openDeviceFolder: async () => {},
  createDevicePlaylist: (url, format, target, numbered) =>
    createPlaylist(url, format, [target.deviceName, target.path].filter(Boolean).join(' / '), numbered, target),
  copyToDevice: async (blockKey, _target, onProgress) => (await walkFiles(blockKey, onProgress)).result,
  copyFromDevice: copyPlaylist,
  scanDeviceImport: async (deviceId, deviceName, root) => ({
    found: 1,
    candidates: [{
      folder: 'Para el coche (mp3)', title: 'Para el coche', format: 'MP3',
      target: { deviceId, deviceName, path: [root, 'Para el coche (mp3)'].filter(Boolean).join('/') },
    }],
  }),
  importFromDevice: async (targets, onProgress) => targets.map((target, i) => {
    onProgress({ done: i + 1, total: targets.length, file: target.path });
    return {
      folder: target.path.split('/').pop() ?? target.path,
      result: { title: 'Para el coche', format: 'mp3', total: 8, downloaded: 8, pending: 0 },
      error: null,
      alreadyRegistered: false,
    };
  }),
  // A browser has no tools to break: their health is always fine.
  watchToolHealth: () => {},
  ...mockTools,
  appVersion: async () => '0.0.0-mock',
  // Always a newer one, so the window and its states can be seen; installing
  // it ends the way a development build does: nothing to replace.
  checkAppUpdate: async () => ({ ...MOCK_UPDATE }),
  installAppUpdate: async onProgress => {
    for (const percent of [0, 35, 70, 100]) {
      onProgress({ phase: 'downloading', version: MOCK_UPDATE.version, percent });
      await new Promise(resolve => setTimeout(resolve, 150));
    }
    throw { key: 'update.devBuild', args: [] };
  },
  onAppUpdate: () => {},
  // No pictures in a plain browser: every cover stays its gradient tile.
  fileUrl: path => path,
  thumbnail: async () => null,
  onThumbnailReady: () => {},
  onFileDownloaded: listener => { onFileDownloaded = listener; },
  pickCoverImage: async () => null,
  setCover: async () => {},
  firstVideoId: async () => null,
  // Kept for the session only: a browser reload starts with nothing loaded.
  playerMemory: async () => ({ ...playerMemory }),
  savePlayerMemory: async memory => { playerMemory = { ...memory }; },
  // Kept in sessionStorage: a reload keeps the look, like settings.json does.
  preferences: async () => ({ ...readPreferences() }),
  savePreferences: async patch => { writePreferences({ ...readPreferences(), ...patch }); },
  resetPreferences: async () => {
    writePreferences(FACTORY_PREFERENCES);
    return { ...FACTORY_PREFERENCES };
  },
  pickCookiesFile: async () => 'C:\\Users\\Example\\cookies.txt',
  // The made-up cookies file is there; any other path is not.
  fileExists: async path => path.endsWith('cookies.txt'),
  // A plain browser has no window to resize: the page's own full screen stands in.
  setFullscreen: on => {
    if (on && !document.fullscreenElement) void document.documentElement.requestFullscreen().catch(() => {});
    if (!on && document.fullscreenElement) void document.exitFullscreen().catch(() => {});
  },
  onClosing: () => {},
};

const MOCK_UPDATE = {
  version: 'v9.9.9',
  notes: 'NOVEDADES\n\n• Un ejemplo de lo que trae una versión nueva.\n• Las notas llegan ya como texto plano.',
};

let playerMemory: PlayerMemory = { volume: 0.7, last: null };

const FACTORY_PREFERENCES: Preferences = {
  appearance: 'dark', language: 'es', uiScale: 100, playerPlacement: 'both', downloadThumbnails: false,
  includeIdInTitleName: false, appUpdateCheckEnabled: true, autoUpdates: true, cookiesFile: '', lastAppUpdateCheck: '',
};
const PREFERENCES_KEY = 'soundify.mock.preferences';

function readPreferences(): Preferences {
  try {
    const saved = sessionStorage.getItem(PREFERENCES_KEY);
    return saved ? { ...FACTORY_PREFERENCES, ...JSON.parse(saved) as Partial<Preferences> } : FACTORY_PREFERENCES;
  } catch {
    return FACTORY_PREFERENCES;
  }
}

function writePreferences(preferences: Preferences): void {
  try { sessionStorage.setItem(PREFERENCES_KEY, JSON.stringify(preferences)); } catch { /* no storage: just not remembered */ }
}

const MOCK_PHONE: DeviceTarget = { deviceId: 'mock-phone', deviceName: 'Pixel 8', path: '' };
/** The phone's folders, by "/"-joined path ("" = its root). */
const PHONE_FOLDERS: Record<string, string[]> = {
  '': ['Alarms', 'DCIM', 'Download', 'Music'],
  Music: ['Soundify', 'WhatsApp Audio'],
  'Music/Soundify': ['Para el coche (mp3)'],
};

let copyCancelled = false;
let onNotice: (notice: DownloadNotice) => void = () => {};
let onFileDownloaded: () => void = () => {};

/** Walks a counter over the block's files, one every 150 ms, so the progress overlay can be seen. */
async function walkFiles(blockKey: string, onProgress: (progress: CopyProgress) => void): Promise<{ result: ExportResult; title: string }> {
  copyCancelled = false;
  const playlist = playlists.find(p => p.formats.some(b => b.key === blockKey));
  const files = playlist?.formats.find(b => b.key === blockKey)?.tracks.filter(tr => tr.status === 'ok') ?? [];
  let copied = 0;
  for (const track of files) {
    if (copyCancelled) break;
    await new Promise(resolve => window.setTimeout(resolve, 150));
    copied++;
    onProgress({ done: copied, total: files.length, file: track.title });
  }
  const cancelled = copyCancelled;
  return {
    result: { copied: cancelled ? 0 : copied, skipped: 0, missing: 0, cancelled, undone: cancelled ? copied : 0 },
    title: playlist?.title ?? 'playlist',
  };
}

async function copyPlaylist(blockKey: string, destination: string, register: boolean,
                            onProgress: (progress: CopyProgress) => void): Promise<CopyOutcome> {
  const { result, title } = await walkFiles(blockKey, onProgress);
  return {
    result,
    folder: `${destination}\\${title}`,
    register: !register || result.cancelled ? 'notRequested' : 'registered',
  };
}

/** Takes the block out of the library; with `files`, as if every file on disk had been deleted. */
async function deletePlaylistFormat(blockKey: string, files: boolean): Promise<DeleteResult | null> {
  const playlist = playlists.find(p => p.formats.some(b => b.key === blockKey));
  const block = playlist?.formats.find(b => b.key === blockKey);
  if (!playlist || !block) return files ? { deleted: 0, unregistered: 0, leftOver: 0 } : null;
  await new Promise(resolve => window.setTimeout(resolve, 600));
  playlist.formats = playlist.formats.filter(b => b !== block);
  if (playlist.formats.length === 0) playlists.splice(playlists.indexOf(playlist), 1);
  return files ? { deleted: block.tracks.filter(tr => tr.status === 'ok').length, unregistered: 0, leftOver: 0 } : null;
}

/** Registers an empty playlist with a made-up title, as if YouTube had listed 12 videos. */
async function createPlaylist(_url: string, format: string, folder: string, numbered: boolean, device?: DeviceTarget): Promise<CreatedPlaylist> {
  await new Promise(resolve => window.setTimeout(resolve, 600));
  const id = `created-${playlists.length}`;
  const blockKey = `${id}/${format.toLowerCase()}`;
  playlists.push({
    id, title: 'Playlist nueva', folder: `${folder}\\Playlist nueva (${format.toLowerCase()})`,
    formats: [{ key: blockKey, playlistId: id, format, video: VIDEO_FORMATS.includes(format), numbered, sync: false, lastSync: '', device, tracks: [] }],
  });
  return { blockKey, title: 'Playlist nueva', count: 12, genericTitle: false, numberMode: { numberFiles: numbered, source: 'requested' } };
}

function startDownload(request: DownloadRequest): void {
  if (ui.task) return;
  const { title, origin, total, blockKey } = describe(request);
  ui.task = { title, origin, step: 'prepare', fraction: null, total, blockKey };
  ui.unseenResult = null;
  emit('tasks');
  let ticks = 0;
  timer = window.setInterval(() => {
    const task = ui.task;
    if (!task) return;
    ticks++;
    if (task.step === 'prepare') {
      if (ticks >= 3) {
        task.step = 'download';
        task.fraction = 0;
      }
    } else if ((task.fraction ?? 0) < 1) {
      task.fraction = Math.min(1, (task.fraction ?? 0) + 1 / task.total);
      landOneFile(task.blockKey);
    } else {
      finish('ok');
      return;
    }
    emit('tasks');
  }, 700);
}

/** A pending file of the block comes down, as the archive would record it, and the front hears so. */
function landOneFile(blockKey: string | null): void {
  const block = playlists.flatMap(p => p.formats).find(b => b.key === blockKey);
  const track = block?.tracks.find(tr => tr.status === 'pending');
  if (!track) return;
  track.status = 'ok';
  onFileDownloaded();
}

/** What the task window shows for a request, and how many files the counter goes up to. */
function describe(request: DownloadRequest): { title: string; origin: TaskOrigin; total: number; blockKey: string | null } {
  switch (request.kind) {
    case 'single':
    case 'single-to-device': return { title: request.url, origin: 'url', total: 1, blockKey: null };
    case 'update': {
      const playlist = playlists.find(p => p.formats.some(b => b.key === request.blockKey));
      const block = playlist?.formats.find(b => b.key === request.blockKey);
      return {
        title: playlist?.title ?? request.blockKey,
        origin: 'manager',
        // What is missing, as the real one asks yt-dlp for; a block with nothing missing still walks one.
        total: Math.max(1, block?.tracks.filter(tr => tr.status === 'pending').length ?? 0),
        blockKey: request.blockKey,
      };
    }
  }
}

function finish(status: TaskStatus): void {
  window.clearInterval(timer);
  const task = ui.task;
  if (!task) return;
  const now = new Date();
  const time = `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`;
  ui.history.unshift({ title: task.title, origin: task.origin, status, summary: null, summaryArgs: [], finishedAt: time });
  ui.history = ui.history.slice(0, 20);
  ui.task = null;
  ui.unseenResult = status;
  emit('tasks');
  if (status === 'ok') onNotice({ status: 'OK', key: 'tasks.status.ok', args: [] });
}
