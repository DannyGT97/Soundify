// «Elegir carpeta» / «Elegir imagen» / «Elegir cookies» for the screens: the app's own explorer on
// desktop, the phone's file chooser on Android (until the explorer gets there: it is built on real
// paths, which is what Android works with, but its listing comes from Kotlin).

import { api } from './api';
import { onAndroid } from './platform';
import { openExplorer, type ExplorerPick, type ExplorerPurpose } from './overlays/explorer';

/** Opens the explorer and answers with what was picked, or null if the window closed without it. */
function ask(purpose: ExplorerPurpose, title: string): Promise<ExplorerPick | null> {
  return new Promise(resolve => openExplorer(purpose, resolve, { title, onCancel: () => resolve(null) }));
}

/** A folder of this computer: where a copy goes, or where to look for playlists to import. */
export async function pickLocalFolder(purpose: 'copy-folder' | 'import-folder', title: string): Promise<string | null> {
  if (onAndroid) return api.pickFolder(title);
  const picked = await ask(purpose, title);
  return picked && 'path' in picked ? picked.path : null;
}

/** An image for a cover. The page is let to show it. */
export async function pickCoverFile(title: string, filter: string): Promise<string | null> {
  if (onAndroid) return api.pickCoverImage(title, filter);
  const picked = await ask('cover', title);
  if (!picked || !('path' in picked)) return null;
  await api.allowPickedFile(picked.path);
  return picked.path;
}

/** The cookies.txt exported from the browser. */
export async function pickCookiesFile(title: string, filter: string): Promise<string | null> {
  if (onAndroid) return api.pickCookiesFile(title, filter);
  const picked = await ask('cookies', title);
  return picked && 'path' in picked ? picked.path : null;
}
