// The two flows of PlaylistTransfer as the Gestor offers them: "Copiar a una
// carpeta local" and "Importar". The texts come from `../transfer.ts` (pure,
// tested); here only the order of the steps and the windows.

import { h } from '../dom';
import { t } from '../i18n';
import { api, type FormatBlock, type Playlist } from '../api';
import { loadLibrary } from '../library';
import { closeOverlay, openOverlay } from '../components/overlay';
import { notify } from '../components/toasts';
import { confirmDialog } from './confirm';
import {
  bulletList, copySummary, errorText, folderName, importSummary, nothingFound, nothingNew,
} from '../transfer';

/** Folder chooser → confirmation (with "register too") → progress with Cancel → result. */
export async function copyToFolder(playlist: Playlist, block: FormatBlock): Promise<void> {
  const format = block.format.toUpperCase();
  const destination = await api.pickFolder(t('manager.copy.chooseTarget', playlist.title, format));
  if (!destination) return;

  const register = h('input', { attrs: { type: 'checkbox' } });
  register.checked = true;
  confirmDialog(t('manager.copy.title'), t('manager.copy.confirmHeader', playlist.title, format, destination),
    t('manager.copy.body'),
    [{ label: t('manager.copy.confirm'), kind: 'primary', run: () => void runCopy(playlist, block, destination, register.checked) }],
    h('label', { class: 'check' }, register, h('span', { text: t('manager.copy.registerCheckbox') })));
}

async function runCopy(playlist: Playlist, block: FormatBlock, destination: string, register: boolean): Promise<void> {
  const fill = h('div', { class: 'progress-fill' });
  const bar = h('div', { class: 'progress indeterminate' }, fill);
  const status = h('div', { class: 'row-title ellipsis', text: t('transfer.preparing') });
  const cancel = h('button', { class: 'secondary-button danger-text', text: t('transfer.cancel'), onclick: () => {
    cancel.disabled = true;
    status.textContent = t('transfer.cancelling');
    api.cancelCopy();
  } });
  // Closing the window by hand would leave the copy running unseen: it cancels it instead.
  let finished = false;
  openOverlay(t('manager.copy.progressTitle', playlist.title, block.format.toUpperCase()),
    [h('div', { class: 'stack-12' }, bar, status, h('div', { class: 'row end' }, cancel))],
    { onClose: () => { if (!finished) api.cancelCopy(); } });

  try {
    const outcome = await api.copyPlaylist(block.key, destination, register, progress => {
      if (cancel.disabled) return;
      bar.classList.remove('indeterminate');
      fill.style.width = `${progress.total ? Math.round(progress.done * 100 / progress.total) : 100}%`;
      status.textContent = t('transfer.progress', progress.done, progress.total, progress.file);
    });
    notify(copySummary(outcome));
    if (outcome.register === 'registered') void loadLibrary();
  } catch (error) {
    notify({ tone: 'error', text: t('manager.copy.failed', errorText(error)) });
  } finally {
    finished = true;
    closeOverlay();
  }
}

/** Folder chooser → scan → confirmation if there is more than one → import → result. */
export async function importFromFolder(): Promise<void> {
  const folder = await api.pickFolder(t('manager.import.chooseFolder'));
  if (!folder) return;
  try {
    const scan = await api.scanImport(folder);
    if (scan.found === 0) return notify(nothingFound(folderName(folder)));
    if (scan.candidates.length === 0) return notify(nothingNew(scan.found));

    const omitted = scan.found - scan.candidates.length;
    const folders = scan.candidates.map(c => c.folder);
    // One is imported straight away: asking to confirm a single, already chosen folder is a step in the way.
    if (folders.length === 1) return await runImport(folders, omitted);
    confirmDialog(t('manager.import.confirmTitle'), t('manager.import.confirmHeader', folders.length),
      t('manager.import.confirmBody', bulletList(scan.candidates),
        omitted > 0 ? t('manager.import.confirmOmitted', omitted) : ''),
      [{ label: t('manager.import.confirmButton'), kind: 'primary', run: () => void runImport(folders, omitted) }]);
  } catch (error) {
    notify({ tone: 'error', text: t('manager.import.noneImported', errorText(error)) });
  }
}

async function runImport(folders: string[], omitted: number): Promise<void> {
  const outcomes = await api.importPlaylists(folders);
  await loadLibrary();
  notify(importSummary(outcomes, omitted));
}
