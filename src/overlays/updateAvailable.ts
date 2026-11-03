import { h } from '../dom';
import { t } from '../i18n';
import { closeOverlay, openOverlay } from '../components/overlay';
import type { AppUpdate } from '../api';

/**
 * «Hay una versión nueva»: what it brings and the decision right below. The
 * notes are written by whoever publishes, i.e. they come from outside: they
 * go in as TEXT, never as markup. Not a wide window: that one takes the
 * whole height, and a few lines of notes were left above an empty panel.
 */
export function openUpdateAvailable(update: AppUpdate, installed: string, onInstall: () => void): void {
  openOverlay(t('update.available.heading', update.version), [
    h('p', { class: 'subtitle', text: t('update.available.from', installed) }),
    // Always there, installing or not: it is what sets this apart from
    // "download an exe from the internet".
    h('p', { class: 'help-note', text: t('update.available.trust') }),
    // Read like the rest of the window, not like the activity log: they are
    // written for whoever updates, not for whoever debugs.
    h('div', { class: 'update-notes', text: update.notes.trim() || t('update.available.noNotes') }),
    h('div', { class: 'row center-x gap-12' },
      // Closed first: installing closes the whole app.
      h('button', { class: 'primary-button', text: t('update.available.install'), onclick: () => {
        closeOverlay();
        onInstall();
      } }),
      h('button', { class: 'secondary-button', text: t('update.available.later'), onclick: () => closeOverlay() })),
  ]);
}
