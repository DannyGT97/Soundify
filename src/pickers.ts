// «Elegir carpeta» / «Elegir imagen» / «Elegir cookies» for the screens: always the app's own explorer
// (on Android too: the music lives in real folders there). The system's chooser stays one button away
// inside the explorer, as «Explorador del sistema…».

import { api } from './api';
import { openExplorer, type ExplorerPick, type ExplorerPurpose } from './overlays/explorer';

/** Opens the explorer and answers with what was picked, or null if the window closed without it. */
function ask(purpose: ExplorerPurpose, title: string): Promise<ExplorerPick | null> {
  return new Promise(resolve => openExplorer(purpose, resolve, { title, onCancel: () => resolve(null) }));
}

/** A folder of this computer where a copy goes. */
export async function pickLocalFolder(purpose: 'copy-folder', title: string): Promise<string | null> {
  const picked = await ask(purpose, title);
  return picked && 'path' in picked ? picked.path : null;
}

/** The folders to import playlists from: one the user pointed at, or the several found by «Buscar en subcarpetas». */
export async function pickImportFolders(title: string): Promise<string[] | null> {
  const picked = await ask('import-folder', title);
  if (!picked) return null;
  if ('paths' in picked) return picked.paths;
  return 'path' in picked ? [picked.path] : null;
}

/** An image for a cover. The page is let to show it. */
export async function pickCoverFile(title: string): Promise<string | null> {
  const picked = await ask('cover', title);
  if (!picked || !('path' in picked)) return null;
  await api.allowPickedFile(picked.path);
  return picked.path;
}

/** The cookies.txt exported from the browser. */
export async function pickCookiesFile(title: string): Promise<string | null> {
  const picked = await ask('cookies', title);
  return picked && 'path' in picked ? picked.path : null;
}
