// Access to the phone's shared storage (Android). The music lives in real
// folders (`Music/Soundify`), so the app needs «All files access»; until it is
// given, the library cannot be read and nothing can be downloaded. Everywhere
// else this is always granted.

import { api } from './api';
import type { StorageInfo } from './api/types';
import { loadLibrary } from './library';
import { emit } from './state';

export const storage: { info: StorageInfo | null } = { info: null };

/** True only once we know the app may NOT read the phone's folders. */
export function needsStorageAccess(info: StorageInfo | null = storage.info): boolean {
  return info !== null && !info.allFiles;
}

/** The folder a new download goes to unless another is chosen ('' where there is no default). */
export function defaultMusicDir(): string {
  return storage.info?.musicDir ?? '';
}

export async function loadStorage(): Promise<void> {
  storage.info = await api.storageInfo();
  emit('storage');
}

/** Opens the system screen and, if they grant it, reads the library again. */
export async function grantStorage(): Promise<void> {
  await api.requestAllFiles();
  await loadStorage();
  if (!needsStorageAccess()) await loadLibrary();
}
