// PLACEHOLDER API: the library comes from data.ts and a download only advances
// a counter, so the menu's indicator and the tasks window can be seen in every
// state. The real one is fed by the yt-dlp process (its --print hooks and
// progress lines) and keeps the same contract: it fills `ui` and emits.

import type { Api } from '..';
import type { CopyOutcome, CopyProgress, DownloadRequest, TaskOrigin } from '../types';
import { emit, ui, type TaskStatus } from '../../state';
import { activityLog, AUDIO_FORMATS, playlists, VIDEO_FORMATS } from './data';

let timer: number | undefined;

export const mockApi: Api = {
  listPlaylists: async () => playlists,
  formats: async () => ({ audio: AUDIO_FORMATS, video: VIDEO_FORMATS }),
  startDownload,
  stopDownload: () => { if (ui.task) finish('stopped'); },
  clearTaskHistory: () => {
    ui.history = [];
    emit('tasks');
  },
  activityLog: async () => activityLog,
  // There is no folder chooser in a plain browser: always the same made-up one.
  pickFolder: async () => 'D:\\Música\\Descargas',
  copyPlaylist,
  cancelCopy: () => { copyCancelled = true; },
  scanImport: async folder => ({
    found: 2,
    candidates: [{ folder: `${folder}\\Lo-fi para estudiar`, title: 'Lo-fi para estudiar', format: 'mp3' }],
  }),
  importPlaylists: async folders => folders.map(folder => ({
    folder,
    result: { title: 'Lo-fi para estudiar', format: 'mp3', total: 12, downloaded: 10, pending: 2 },
    error: null,
    alreadyRegistered: false,
  })),
  revealFolder: () => {},
};

let copyCancelled = false;

/** Walks a counter over the block's files, one every 150 ms, so the progress overlay can be seen. */
async function copyPlaylist(blockKey: string, destination: string, register: boolean,
                            onProgress: (progress: CopyProgress) => void): Promise<CopyOutcome> {
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
    folder: `${destination}\\${playlist?.title ?? 'playlist'}`,
    register: !register || cancelled ? 'notRequested' : 'registered',
  };
}

function startDownload(request: DownloadRequest): void {
  if (ui.task) return;
  const { title, origin, total } = describe(request);
  ui.task = { title, origin, step: 'prepare', done: 0, total };
  ui.unseenResult = null;
  emit('tasks');
  let ticks = 0;
  timer = window.setInterval(() => {
    const task = ui.task;
    if (!task) return;
    ticks++;
    if (task.step === 'prepare') {
      if (ticks >= 3) task.step = 'download';
    } else if (task.done < task.total) {
      task.done++;
    } else {
      finish('ok');
      return;
    }
    emit('tasks');
  }, 700);
}

/** What the task window shows for a request, and how many files the counter goes up to. */
function describe(request: DownloadRequest): { title: string; origin: TaskOrigin; total: number } {
  switch (request.kind) {
    case 'single': return { title: request.url, origin: 'url', total: 1 };
    case 'new-playlist': return { title: request.url, origin: 'manager', total: 12 };
    case 'update': {
      const playlist = playlists.find(p => p.formats.some(b => b.key === request.blockKey));
      const block = playlist?.formats.find(b => b.key === request.blockKey);
      return {
        title: playlist?.title ?? request.blockKey,
        origin: 'manager',
        total: block?.tracks.filter(tr => tr.status === 'ok').length ?? 1,
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
  ui.history.unshift({ title: task.title, origin: task.origin, status, finishedAt: time });
  ui.history = ui.history.slice(0, 20);
  ui.task = null;
  ui.unseenResult = status;
  emit('tasks');
}
