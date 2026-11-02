// The real API, over Tauri's invoke(). What a phase has not moved to the back
// yet still goes to the mock, so every screen keeps working in between.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import type { Api } from '.';
import type { CopyOutcome, CopyProgress, FormatCatalog, ImportOutcome, ImportScan, Playlist } from './types';
import { mockApi } from './mock';
import { emit, ui } from '../state';

export const tauriApi: Api = {
  listPlaylists: () => invoke<Playlist[]>('list_playlists'),
  formats: () => invoke<FormatCatalog>('formats'),
  // Phase 5 (download engine).
  startDownload: request => mockApi.startDownload(request),
  stopDownload: () => mockApi.stopDownload(),
  clearTaskHistory: () => {
    ui.history = [];
    emit('tasks');
    void invoke('clear_task_history');
  },
  activityLog: () => mockApi.activityLog(),
  pickFolder: async title => {
    const chosen = await open({ directory: true, multiple: false, title });
    return typeof chosen === 'string' ? chosen : null;
  },
  copyPlaylist: async (blockKey, destination, register, onProgress) => {
    const stop = await listen<CopyProgress>('copy-progress', event => onProgress(event.payload));
    try {
      return await invoke<CopyOutcome>('copy_playlist', { blockKey, destination, register });
    } finally {
      stop();
    }
  },
  cancelCopy: () => void invoke('cancel_copy'),
  scanImport: folder => invoke<ImportScan>('scan_import', { folder }),
  importPlaylists: folders => invoke<ImportOutcome[]>('import_playlists', { folders }),
  revealFolder: folder => void revealItemInDir(folder),
};

/** Inside the Tauri window, not in a plain browser running `npm run dev`. */
export function insideTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
