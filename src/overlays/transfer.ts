// The flows of PlaylistTransfer as the Gestor offers them: "Copiar playlist"
// (to a folder, to a phone, or from a phone down to this computer) and
// "Importar" (from a folder or from a phone). The texts come from
// `../transfer.ts` (pure, tested); here only the order of the steps and the windows.

import { h } from '../dom';
import { t } from '../i18n';
import {
  api, type CopyOutcome, type CopyProgress, type DeviceTarget, type FormatBlock, type ImportCandidate, type Playlist,
} from '../api';
import { loadLibrary } from '../library';
import { closeOverlay, openOverlay } from '../components/overlay';
import { notify } from '../components/toasts';
import { confirmDialog } from './confirm';
import { deviceLabel, openExplorer } from './explorer';
import { pickImportFolders, pickLocalFolder } from '../pickers';
import {
  bulletList, copySummary, errorText, folderName, importSummary, nothingFound, nothingNew,
} from '../transfer';

type Copy = (onProgress: (progress: CopyProgress) => void) => Promise<CopyOutcome>;

/**
 * Folder chooser → confirmation (with "register too") → progress with Cancel
 * → result. A playlist that lives on a phone comes down to this computer the
 * same way.
 */
export async function copyToFolder(playlist: Playlist, block: FormatBlock): Promise<void> {
  const format = block.format.toUpperCase();
  const destination = await pickLocalFolder('copy-folder', t('manager.copy.chooseTarget', playlist.title, format));
  if (!destination) return;

  const register = h('input', { attrs: { type: 'checkbox' } });
  register.checked = true;
  const copy: Copy = onProgress => block.device
    ? api.copyFromDevice(block.key, destination, register.checked, onProgress)
    : api.copyPlaylist(block.key, destination, register.checked, onProgress);
  confirmDialog(t('manager.copy.title'), t('manager.copy.confirmHeader', playlist.title, format, destination),
    t('manager.copy.body'),
    [{ label: t('manager.copy.confirm'), kind: 'primary', run: () => void runCopy(playlist, block, copy) }],
    h('label', { class: 'check' }, register, h('span', { text: t('manager.copy.registerCheckbox') })));
}

/** «Copiar a un dispositivo…»: the picker, then the same progress as a folder copy. */
export function copyToDevice(playlist: Playlist, block: FormatBlock): void {
  openExplorer('copy-device', picked => {
    if (!('device' in picked)) return;
    const target = picked.device;
    void runCopy(playlist, block, async onProgress => ({
      result: await api.copyToDevice(block.key, target, onProgress),
      folder: deviceLabel(target),
      register: 'notRequested',
    }));
  }, { subject: { playlist, block } });
}

async function runCopy(playlist: Playlist, block: FormatBlock, copy: Copy): Promise<void> {
  const progress = progressWindow(t('manager.copy.progressTitle', playlist.title, block.format.toUpperCase()),
    t('transfer.preparing'), true);
  try {
    const outcome = await copy(progress.update);
    notify(copySummary(outcome));
    if (outcome.register === 'registered') void loadLibrary();
  } catch (error) {
    notify({ tone: 'error', text: t('manager.copy.failed', errorText(error)) });
  } finally {
    progress.close();
  }
}

/** Folder chooser → scan → confirmation if there is more than one → import → result. */
export async function importFromFolder(): Promise<void> {
  const chosen = await pickImportFolders(t('manager.import.chooseFolder'));
  if (!chosen?.length) return;
  try {
    // One folder the user pointed at, or several playlists found below one: each is looked at on its own.
    const scans = await Promise.all(chosen.map(folder => api.scanImport(folder)));
    const found = scans.reduce((sum, scan) => sum + scan.found, 0);
    const seen = new Set<string>();
    const candidates = scans.flatMap(scan => scan.candidates).filter(c => !seen.has(c.folder) && seen.add(c.folder));
    if (found === 0) { notify(nothingFound(folderName(chosen[0] ?? ''))); return; }
    if (candidates.length === 0) { notify(nothingNew(found)); return; }

    const omitted = found - candidates.length;
    const folders = candidates.map(c => c.folder);
    await confirmImport(candidates, omitted, async () => {
      const outcomes = await api.importPlaylists(folders);
      await loadLibrary();
      notify(importSummary(outcomes, omitted));
    });
  } catch (error) {
    notify({ tone: 'error', text: t('manager.import.noneImported', errorText(error)) });
  }
}

/**
 * Device picker → scan (a window says so: every subfolder is a round trip to
 * the phone) → confirmation if there is more than one → import, with a count
 * per manifest pulled → result.
 */
export function importFromDevice(): void {
  openExplorer('import-device', picked => {
    if ('device' in picked) void scanDevice(picked.device);
  });
}

async function scanDevice(target: DeviceTarget): Promise<void> {
  const scanning = progressWindow(t('manager.importDevice.windowTitle'), t('manager.importDevice.scanning'), false);
  try {
    const scan = await api.scanDeviceImport(target.deviceId, target.deviceName, target.path);
    scanning.close();
    if (scan.found === 0) {
      notify(nothingFound(target.path ? folderName(target.path) : t('explorer.root')));
      return;
    }
    if (scan.candidates.length === 0) { notify(nothingNew(scan.found)); return; }

    const omitted = scan.found - scan.candidates.length;
    const targets = scan.candidates.map(c => c.target);
    await confirmImport(scan.candidates, omitted, async () => {
      const importing = progressWindow(t('manager.importDevice.windowTitle'), t('manager.importDevice.importingTitle'), false);
      try {
        const outcomes = await api.importFromDevice(targets, importing.update);
        await loadLibrary();
        notify(importSummary(outcomes, omitted));
      } finally {
        importing.close();
      }
    });
  } catch (error) {
    scanning.close();
    notify({ tone: 'error', text: t('manager.import.noneImported', errorText(error)) });
  }
}

/** One is imported straight away: asking to confirm a single, already chosen folder is a step in the way. */
async function confirmImport(candidates: ImportCandidate[], omitted: number, run: () => Promise<void>): Promise<void> {
  if (candidates.length === 1) return run();
  confirmDialog(t('manager.import.confirmTitle'), t('manager.import.confirmHeader', candidates.length),
    t('manager.import.confirmBody', bulletList(candidates),
      omitted > 0 ? t('manager.import.confirmOmitted', omitted) : ''),
    [{ label: t('manager.import.confirmButton'), kind: 'primary', run: () => void run().catch(error =>
      notify({ tone: 'error', text: t('manager.import.noneImported', errorText(error)) })) }]);
}

interface ProgressWindow {
  update: (progress: CopyProgress) => void;
  close: () => void;
}

/**
 * A bar (moving with no number until the first count arrives) and a line of
 * text. With `cancellable`, a Cancel that stops the copy between two files;
 * closing the window by hand would leave the copy running unseen, so that
 * cancels it too.
 */
function progressWindow(title: string, text: string, cancellable: boolean): ProgressWindow {
  const fill = h('div', { class: 'progress-fill' });
  const bar = h('div', { class: 'progress indeterminate' }, fill);
  const status = h('div', { class: 'row-title ellipsis', text });
  const cancel = h('button', { class: 'secondary-button danger-text', text: t('transfer.cancel'), onclick: () => {
    cancel.disabled = true;
    status.textContent = t('transfer.cancelling');
    api.cancelCopy();
  } });
  let finished = false;
  const body = h('div', { class: 'stack-12' }, bar, status);
  if (cancellable) body.append(h('div', { class: 'row end' }, cancel));
  openOverlay(title, [body], { onClose: () => { if (!finished && cancellable) api.cancelCopy(); } });
  return {
    update: progress => {
      if (cancel.disabled) return;
      bar.classList.remove('indeterminate');
      fill.style.width = `${progress.total ? Math.round(progress.done * 100 / progress.total) : 100}%`;
      status.textContent = t('transfer.progress', progress.done, progress.total, progress.file);
    },
    close: () => {
      if (finished) return;
      finished = true;
      closeOverlay();
    },
  };
}
