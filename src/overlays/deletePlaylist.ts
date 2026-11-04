import { h } from '../dom';
import { icon, type IconName } from '../icons';
import { t } from '../i18n';
import { api, type FormatBlock, type Playlist } from '../api';
import { filesText, loadLibrary, onDisk } from '../library';
import { cover } from '../components/cover';
import { closeButton, closeOverlay, openOverlay, openPanel } from '../components/overlay';
import { notify } from '../components/toasts';
import { playlistPicture } from '../covers';
import { errorText } from '../transfer';
import { deviceLabel } from './devicePicker';

/** Deleting can be over before the eye catches it: the window stays at least this long. */
const MIN_DELETE_WINDOW_MS = 600;

type Choice = 'record' | 'all';

/**
 * Eliminar a playlist's format: forget it, or forget it and empty its folder.
 * With no reachable folder, «Todo» is NOT offered: it would find nothing to
 * delete and still say it had.
 */
export function openDeleteWindow(playlist: Playlist, block: FormatBlock): void {
  const location = block.device ? deviceLabel(block.device) : playlist.folder;
  const unreachable = block.accessible === false;
  const files = onDisk(block.tracks).length;
  let choice: Choice = 'record';

  const consequence = (kind: 'gone' | 'kept', text: string, iconName: IconName = kind === 'gone' ? 'close' : 'ok') =>
    h('div', { class: kind }, icon(iconName, 14), h('span', { text }));

  const record = h('button', { class: 'option', onclick: () => choose('record') },
    h('span', { class: 'radio' }),
    h('b', { text: t('delete.record') }),
    h('span', { class: 'conseq' },
      consequence('kept', t('delete.record.kept', filesText(files))),
      consequence('gone', t('delete.record.gone'))),
    h('small', { text: t('delete.record.note') }));
  const everything = h('button', { class: 'option danger', onclick: () => choose('all') },
    h('span', { class: 'radio' }),
    h('b', { text: t('delete.all') }),
    h('span', { class: 'conseq' },
      consequence('gone', t('delete.all.files', filesText(files))),
      consequence('gone', t('delete.all.rest')),
      consequence('gone', t('delete.all.record'))),
    unreachable ? h('span', { class: 'why', text: t('delete.all.connect') }) : h('small', { text: t('delete.all.note') }));
  everything.disabled = unreachable;

  const whereLabel = h('small');
  const confirm = h('button', { class: 'primary-button' });

  function sync(): void {
    record.classList.toggle('on', choice === 'record');
    everything.classList.toggle('on', choice === 'all');
    whereLabel.textContent = t(choice === 'all' ? 'delete.where.emptied' : 'delete.where.kept');
    confirm.className = choice === 'all' ? 'danger-button' : 'primary-button';
    confirm.textContent = choice === 'all' ? t('delete.confirm.all', filesText(files)) : t('delete.confirm.record');
  }

  function choose(next: Choice): void {
    choice = next;
    sync();
  }

  confirm.addEventListener('click', () => {
    if (choice === 'all') void deleteAll(playlist, block, location);
    else void forget(block);
  });
  sync();

  openPanel(h('div', { class: 'mod' },
    h('div', { class: 'mod-head' },
      h('h2', { class: 'grow', text: t('manager.delete.title') }),
      closeButton()),
    h('div', { class: 'mod-body' },
      h('div', { class: 'subject' },
        cover(playlist.title, block.video, 44, playlistPicture(playlist)),
        h('div', { class: 'min0' },
          h('b', { class: 'ellipsis', text: playlist.title }),
          h('div', { class: 'muted', text: t('delete.subject', block.format.toUpperCase(), filesText(files)) }))),
      unreachable ? h('div', { class: 'notice' }, icon('warning', 16), h('span', { text: t('delete.unreachable', location) })) : null,
      h('div', { class: 'options' }, record, everything),
      h('div', { class: 'where' },
        icon('folder', 16),
        h('div', { class: 'min0' }, whereLabel, h('div', { class: 'ellipsis', text: location, title: location })))),
    h('div', { class: 'mod-foot' },
      h('button', { class: 'secondary-button', text: t('common.cancel'), onclick: () => closeOverlay() }),
      confirm)));
}

async function forget(block: FormatBlock): Promise<void> {
  closeOverlay();
  try {
    await api.deletePlaylistFormat(block.key, false);
  } catch (error) {
    notify({ tone: 'error', text: errorText(error) });
  }
  await loadLibrary();
}

async function deleteAll(playlist: Playlist, block: FormatBlock, location: string): Promise<void> {
  const onDevice = block.device != null;
  openOverlay(t(onDevice ? 'manager.deleteDevice.windowTitle' : 'manager.deleteLocal.windowTitle'), [
    h('div', { class: 'stack-12' },
      h('div', { class: 'progress indeterminate' }, h('div', { class: 'progress-fill' })),
      h('div', { class: 'row-title ellipsis', text: t(onDevice ? 'manager.deleteDevice.inProgress' : 'manager.deleteLocal.inProgress', playlist.title) })),
  ]);
  const started = Date.now();
  try {
    const result = await api.deletePlaylistFormat(block.key, true);
    const wait = MIN_DELETE_WINDOW_MS - (Date.now() - started);
    if (wait > 0) await new Promise(resolve => window.setTimeout(resolve, wait));
    const total = (result?.deleted ?? 0) + (result?.unregistered ?? 0);
    notify(result && result.leftOver > 0
      ? { tone: 'error', text: t('manager.delete.leftOver', playlist.title, total, result.leftOver, location) }
      : { tone: 'success', text: t('manager.delete.done', playlist.title, block.format.toUpperCase(), total) });
  } catch (error) {
    notify({ tone: 'error', text: errorText(error) });
  } finally {
    closeOverlay();
    await loadLibrary();
  }
}
