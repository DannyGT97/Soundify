// The app's own updates as Ajustes and the startup see them (the Java
// `UpdatesSettingsController` and `StartupUpdateCheck`). Checking, verifying
// and swapping live in the back (`core::updater`); this keeps what the panel
// shows and opens the «Hay una versión nueva» window.
//
// RESULTS are toasts and expire; what is IN PROGRESS (the percentage,
// "installing...") stays on the panel's own status line, where it is looked
// for and where it cannot vanish halfway.

import { api, type AppUpdate, type AppUpdateProgress } from './api';
import { appVersion } from './appInfo';
import { t } from './i18n';
import { onAndroid } from './platform';
import { notify } from './components/toasts';
import { openUpdateAvailable } from './overlays/updateAvailable';
import { errorText } from './transfer';

export interface UpdateState {
  /** The version found, while the button offers to install it. */
  pending: AppUpdate | null;
  busy: boolean;
  /** The line under the button; empty when there is nothing in progress. */
  status: string;
  /** Something went wrong: the button says «Buscar ahora» instead of «Buscar actualizaciones». */
  failed: boolean;
}

const state: UpdateState = { pending: null, busy: false, status: '', failed: false };
const listeners = new Set<() => void>();

export function updateState(): Readonly<UpdateState> {
  return state;
}

/** Calls back on every change; returns the function that stops it. */
export function onUpdateChange(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The startup check found one: the window opens by itself, with the notes. */
export function watchAppUpdates(): void {
  api.onAppUpdate(update => {
    change({ pending: update });
    openUpdateAvailable(update, appVersion(), () => void installUpdate());
  });
}

/** «Buscar actualizaciones», or «Descargar e instalar» once one was found. */
export async function checkForUpdate(): Promise<void> {
  if (state.pending) {
    await installUpdate();
    return;
  }
  change({ busy: true, status: t('settings.updates.checking') });
  try {
    const found = await api.checkAppUpdate();
    change({ busy: false, status: '', failed: false, pending: found });
    if (!found) {
      // Green: next to a failure shown in red, neutral read as "something did
      // not quite work".
      notify({ tone: 'success', text: t('settings.updates.upToDate', appVersion().replace(/^v/, '')) });
      return;
    }
    // No toast about it: the window says so, with the notes right there.
    openUpdateAvailable(found, appVersion(), () => void installUpdate());
  } catch (error) {
    fail(t('settings.updates.checkFailed', errorText(error)));
  }
}

/** The text of the status line while it installs. */
export function progressText(progress: AppUpdateProgress): string {
  switch (progress.phase) {
    case 'downloading': return t('settings.updates.downloading', progress.version, progress.percent);
    case 'verified': return t('settings.updates.verified');
    case 'installing': return t(onAndroid ? 'settings.updates.installing.android' : 'settings.updates.installing', progress.version);
  }
}

// === PRIVATE ===

async function installUpdate(): Promise<void> {
  const update = state.pending;
  if (!update || state.busy) return;
  change({ busy: true, status: t('settings.updates.downloading', update.version, 0) });
  try {
    await api.installAppUpdate(progress => change({ status: progressText(progress) }));
    // The app closes from the back; until then, the last line stays.
  } catch (error) {
    // A failed verification lands here too, and it matters most: it is not
    // "could not download", it is that what came was NOT trustworthy.
    fail(t('settings.updates.installFailed', errorText(error)));
  }
}

function fail(message: string): void {
  change({ busy: false, status: '', failed: true, pending: null });
  notify({ tone: 'error', text: message });
}

function change(patch: Partial<UpdateState>): void {
  Object.assign(state, patch);
  listeners.forEach(listener => listener());
}
