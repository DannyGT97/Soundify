import { h, titleRow } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { countBy, onDisk, playlists, type FormatBlock, type Playlist } from '../mock';
import { navigate, onLeave, type Params } from '../router';
import { subscribe, ui } from '../state';
import { startTask } from '../tasks';
import { cover } from '../components/cover';
import { confirmDialog } from '../overlays/confirm';
import { openDevicePicker } from '../overlays/devicePicker';
import { openFilesOverlay } from '../overlays/files';
import { openImportHelp } from '../overlays/help';

/**
 * Gestor de Playlists: one folding row per playlist+folder, with a block per
 * format inside. The state shown is always the last one known: nothing asks
 * YouTube on opening.
 */
export function managerView(params: Params): HTMLElement {
  if (params.open) ui.expandedPlaylists.add(params.open);
  const list = h('div', { class: 'stack-14' });
  const render = () => list.replaceChildren(...playlists.map(playlistRow));
  onLeave(subscribe('tasks', render));
  render();

  const importButton = h('button', { class: 'secondary-button', onclick: openImportHelp },
    icon('import', 16), h('span', { text: t('manager.import') }));

  const empty = h('section', { class: 'card stack-14' },
    h('p', { class: 'subtitle', text: t('manager.empty') }),
    h('p', { class: 'subtitle', text: t('manager.empty.recover') }),
    h('div', { class: 'row center-x' }, h('button', { class: 'secondary-button', text: t('manager.findPlaylists'), onclick: openImportHelp })));

  return h('div', { class: 'screen' },
    h('div', { class: 'screen-head' },
      titleRow('manager', t('manager.title'), h('div', { class: 'grow' }), importButton),
      h('p', { class: 'subtitle', text: t('manager.description') })),
    playlists.length ? list : empty);
}

function playlistRow(playlist: Playlist): HTMLElement {
  const expanded = ui.expandedPlaylists.has(playlist.id);
  const files = playlist.formats.reduce((n, b) => n + onDisk(b.tracks).length, 0);
  const header = h('button', { class: 'pm-row-header', title: t(expanded ? 'manager.row.fold' : 'manager.row.unfold') },
    cover(playlist.title, playlist.formats.every(b => b.video), 56),
    h('div', { class: 'grow min0' },
      h('div', { class: 'card-title ellipsis', text: playlist.title }),
      h('div', { class: 'subtitle ellipsis', text: playlist.folder }),
      h('div', { class: 'subtitle', text: `${t('manager.formats', playlist.formats.map(b => b.format).join(', '))} · ${t('manager.files', files)}` })),
    h('span', { class: `chevron${expanded ? ' open' : ''}` }, icon('chevron', 20)));
  const coverButton = header.querySelector<HTMLElement>('.cover');
  if (coverButton) {
    coverButton.classList.add('pm-cover-button');
    coverButton.setAttribute('role', 'button');
    coverButton.setAttribute('tabindex', '0');
    coverButton.setAttribute('aria-label', 'Cambiar portada');
    coverButton.title = 'Cambiar portada';
    coverButton.addEventListener('click', event => {
      event.stopPropagation();
      confirmDialog('Cambiar portada', `Selecciona una nueva portada para «${playlist.title}».`, '', [
        { label: 'Elegir imagen…', kind: 'primary' },
      ]);
    });
    coverButton.addEventListener('keydown', event => {
      if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); coverButton.click(); }
    });
  }
  header.addEventListener('click', () => {
    if (expanded) ui.expandedPlaylists.delete(playlist.id);
    else ui.expandedPlaylists.add(playlist.id);
    navigate('manager');
  });
  return h('section', { class: 'card pm-row' }, header,
    expanded ? h('div', { class: 'pm-blocks' }, ...playlist.formats.map(b => formatBlock(playlist, b))) : null);
}

function formatBlock(playlist: Playlist, block: FormatBlock): HTMLElement {
  const pending = countBy(block.tracks, 'pending');
  const failed = countBy(block.tracks, 'failed');
  const unavailable = countBy(block.tracks, 'unavailable');
  const status = pending
      ? h('span', { class: 'warn-text', text: t('manager.status.pending', pending) })
      : h('span', { class: 'ok-text', text: t('manager.status.synced') });
  const extra = [
    failed ? t('manager.status.moreFailed', failed) : '',
    unavailable ? t('manager.status.moreUnavailable', unavailable) : '',
  ].filter(Boolean).join(' ');

  const busy = ui.task !== null;
  const update = h('button', { class: 'primary-button', onclick: () => startTask(playlist.title, 'manager', onDisk(block.tracks).length) },
    icon('sync', 16), h('span', { text: 'Sincronizar' }));
  update.disabled = busy;
  const files = h('button', { class: 'secondary-button', onclick: () => openFilesOverlay(playlist, block) },
    icon('eye', 16), h('span', { text: t('manager.viewFiles') }));
  const numbered = h('span', {
    class: `pm-numbered-icon${block.numbered ? '' : ' off'}`,
    title: t(block.numbered ? 'manager.numbered' : 'manager.notNumbered'),
    attrs: { role: 'img', 'aria-label': t(block.numbered ? 'manager.numbered' : 'manager.notNumbered') },
  }, icon(block.numbered ? 'list-ordered' : 'list', 16));

  return h('div', { class: 'pm-block' },
    h('div', { class: 'row center gap-10' },
      h('span', { class: 'chip', text: block.format }),
      h('span', { class: 'row-title', text: t('manager.files', onDisk(block.tracks).length) }),
      h('span', { class: 'subtitle', text: t('manager.lastSync', block.lastSync) }),
      h('div', { class: 'grow' })),
    h('div', { class: 'row center gap-8' }, status, extra ? h('span', { class: 'subtitle', text: extra }) : null,
      h('div', { class: 'grow' }),
      numbered),
    h('div', { class: 'row center gap-8 pm-actions' }, update, files, copyMenu(playlist, block),
    h('button', { class: 'icon-button pm-delete', title: t('manager.menu.delete'), attrs: { 'aria-label': t('manager.menu.delete') }, onclick: () => deleteBlock(playlist, block) }, icon('delete', 17))));
}

function deleteBlock(playlist: Playlist, block: FormatBlock): void {
  const label = `${playlist.title} (${block.format})`;
  confirmDialog(t('manager.delete.title'), t('manager.delete.header', label, block.tracks.length),
        t('manager.delete.body'), [
          { label: t('manager.delete.all'), kind: 'danger' },
          { label: t('manager.delete.onlyRecord') },
        ]);
}

/** Copy destinations live under one compact action, with neutral icons. */
function copyMenu(playlist: Playlist, block: FormatBlock): HTMLElement {
  const menu = h('div', { class: 'popup-menu pm-copy-menu' },
    h('button', { onclick: () => {
      menu.hidden = true;
      confirmDialog(t('manager.copy.title'), t('manager.copy.header', playlist.title, block.format),
        t('manager.copy.body'), [{ label: t('manager.copy.confirm'), kind: 'primary' }]);
    } }, icon('folder-open', 16), h('span', { text: t('manager.menu.copy.local') })),
    h('button', { onclick: () => { menu.hidden = true; openDevicePicker(); } },
      icon('device', 16), h('span', { text: t('manager.menu.copy.device') })));
  menu.hidden = true;
  const button = h('button', { class: 'secondary-button pm-copy-button', onclick: e => {
    e.stopPropagation();
    const open = menu.hidden;
    document.querySelectorAll<HTMLElement>('.pm-copy-menu').forEach(m => { m.hidden = true; });
    menu.hidden = !open;
  } }, icon('copy', 16), h('span', { text: 'Copiar' }));
  return h('div', { class: 'menu-anchor' }, button, menu);
}

document.addEventListener('click', () => {
  document.querySelectorAll<HTMLElement>('.pm-copy-menu').forEach(m => { m.hidden = true; });
});
