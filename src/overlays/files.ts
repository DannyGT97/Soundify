import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import type { FormatBlock, Playlist, Track } from '../api';
import { countBy, findBlock, groupByStatus, onDisk, STATUS_ORDER, statusText } from '../library';
import { cover } from '../components/cover';
import { openOverlay } from '../components/overlay';
import * as player from '../player';
import { isPlayable } from '../player';
import { filterByTitle } from '../search';
import { subscribe, ui } from '../state';

/**
 * "Ver archivos" of the Gestor: every file of one playlist+format and its
 * state. It follows the library while open, so with that playlist
 * downloading each file goes from «Pendiente» to «Descargado» as it lands,
 * with a short flash to see which one it was.
 */
export function openFilesOverlay(playlist: Playlist, opened: FormatBlock): void {
  let block = opened;
  let query = '';
  // Grouped from the start: what is already there first, then what is still missing.
  let grouped = true;
  let known = new Set(onDisk(block.tracks).map(track => track.id));
  let landed = new Set<string>();
  const tbody = h('tbody');
  const counter = h('div', { class: 'subtitle' });
  const legend = h('div', { class: 'row wrap gap-8' });
  const canPlayFormat = isPlayable(block.format);
  const playAllButton = h('button', { class: 'primary-button', onclick: () => player.playAll(block.key) },
    icon('play', 16, true), h('span', { text: t('player.playAll') }));

  const refresh = () => {
    let rows = filterByTitle(block.tracks, query);
    if (grouped) rows = groupByStatus(rows);
    tbody.replaceChildren(...rows.map(trackRow));
    counter.textContent = t('songs.totalFiles', block.tracks.length);
    legend.replaceChildren(...STATUS_ORDER
        .map(status => ({ status, n: countBy(block.tracks, status) }))
        .filter(({ n }) => n > 0)
        .map(({ status, n }) => h('span', { class: 'chip', text: t(`songs.legend.${status}`, n) })));
    playAllButton.disabled = !canPlayFormat || block.tracks.every(track => track.status !== 'ok');
  };

  const reload = () => {
    block = findBlock(opened.key)?.block ?? block;
    const now = onDisk(block.tracks).map(track => track.id);
    landed = new Set(now.filter(id => !known.has(id)));
    known = new Set(now);
    refresh();
    landed.clear();
  };

  const trackRow = (track: Track): HTMLTableRowElement => {
    const current = ui.nowPlaying?.blockKey === block.key && ui.nowPlaying.track.id === track.id;
    const playable = canPlayFormat && track.status === 'ok';
    const action = h('button', {
      class: `file-play-button${current ? ' is-playing' : ''}`,
      title: t(current && ui.playing ? 'player.pause' : 'player.play'),
      onclick: () => {
        if (!playable) return;
        if (current) player.togglePlay();
        else player.play(block.key, track);
      },
    }, icon(current && ui.playing ? 'pause' : 'play', 16, true));
    action.disabled = !playable;
    const row = h('tr', { class: `${current ? 'is-playing' : ''}${landed.has(track.id) ? ' just-landed' : ''}` },
      block.numbered ? h('td', { class: 'num', text: String(track.number).padStart(3, '0') }) : null,
      h('td', { class: 'file-title-cell' },
        h('div', { class: 'row center gap-12 min0' },
          cover(track.title, block.video, 36, { kind: 'thumbnail', id: track.id }),
          h('span', { text: track.title }))),
      h('td', {}, h('span', { class: `status-pill status-${track.status}`, text: statusText(track) })),
      h('td', { class: 'file-action-cell' }, action));
    return row;
  };

  const search = h('input', { class: 'input', attrs: { placeholder: t('songs.searchPlaceholder') } });
  search.addEventListener('input', () => {
    query = search.value;
    refresh();
  });
  const groupBox = h('input', { attrs: { type: 'checkbox' } });
  groupBox.checked = grouped;
  groupBox.addEventListener('change', () => {
    grouped = groupBox.checked;
    refresh();
  });

  const heading = h('div', { class: 'row center gap-12 grow min0' },
    h('span', { class: 'title-icon' }, icon('player', 26)),
    h('div', { class: 'min0' },
      h('div', { class: 'row center gap-8' },
        h('h2', { class: 'overlay-title', text: playlist.title }),
        h('span', { class: 'chip', text: block.format })),
      counter));

  // 'player' also fires several times a second with the position: the rows
  // (thumbnails included) are rebuilt only when what is playing changes.
  let shown = '';
  const playingKey = () => `${ui.nowPlaying?.blockKey}|${ui.nowPlaying?.track.id}|${ui.playing}`;
  const followPlayer = () => {
    if (playingKey() === shown) return;
    shown = playingKey();
    refresh();
  };
  const unsubscribe = [subscribe('player', followPlayer), subscribe('library', reload)];
  shown = playingKey();
  refresh();
  openOverlay(heading, [
    h('div', { class: 'row center gap-8 files-toolbar' }, playAllButton, h('div', { class: 'grow' })),
    h('div', { class: 'row center gap-8' }, h('div', { class: 'grow' }, legend),
      h('label', { class: 'check' }, groupBox, h('span', { text: t('songs.groupByStatus') }))),
    search,
    h('div', { class: 'table-wrap' },
      h('table', { class: 'files-table' },
        h('thead', {}, h('tr', {},
          block.numbered ? h('th', { class: 'num', text: t('songs.column.number') }) : null,
          h('th', { text: t('songs.column.title') }),
          h('th', { text: t('songs.column.status') }),
          h('th', { class: 'file-action-heading', text: t('songs.column.play') }))),
        tbody)),
  ], { wide: true, onClose: () => unsubscribe.forEach(stop => stop()) });
}
