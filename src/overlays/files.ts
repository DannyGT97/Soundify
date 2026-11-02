import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import type { FormatBlock, Playlist, Status, Track } from '../api';
import { countBy, statusText } from '../library';
import { openOverlay } from '../components/overlay';
import * as player from '../player';
import { PLAYABLE } from '../player';
import { subscribe, ui } from '../state';

const STATUS_ORDER: Status[] = ['ok', 'pending', 'failed', 'unavailable'];

/** "Ver archivos" of the Gestor: every file of one playlist+format and its state. */
export function openFilesOverlay(playlist: Playlist, block: FormatBlock): void {
  let query = '';
  let grouped = false;
  const tbody = h('tbody');
  const counter = h('div', { class: 'subtitle' });
  const canPlayFormat = PLAYABLE.has(block.format);
  const playAllButton = h('button', { class: 'primary-button', onclick: () => player.playAll(block.key) },
    icon('play', 16, true), h('span', { text: t('player.playAll') }));
  playAllButton.disabled = !canPlayFormat || block.tracks.every(track => track.status !== 'ok');

  const refresh = () => {
    let rows = block.tracks.filter(tr => tr.title.toLowerCase().includes(query.toLowerCase()));
    if (grouped) rows = [...rows].sort((a, b) => STATUS_ORDER.indexOf(a.status) - STATUS_ORDER.indexOf(b.status));
    tbody.replaceChildren(...rows.map(trackRow));
    counter.textContent = t('songs.totalFiles', block.tracks.length);
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
    const row = h('tr', { class: current ? 'is-playing' : '' },
      block.numbered ? h('td', { class: 'num', text: String(track.number).padStart(3, '0') }) : null,
      h('td', { class: 'file-title-cell', text: track.title }),
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
  groupBox.addEventListener('change', () => {
    grouped = groupBox.checked;
    refresh();
  });

  const legend = h('div', { class: 'row wrap gap-8' }, ...STATUS_ORDER
      .map(status => ({ status, n: countBy(block.tracks, status) }))
      .filter(({ n }) => n > 0)
      .map(({ status, n }) => h('span', { class: 'chip', text: t(`songs.legend.${status}`, n) })));

  const heading = h('div', { class: 'row center gap-12 grow min0' },
    h('span', { class: 'title-icon' }, icon('player', 26)),
    h('div', { class: 'min0' },
      h('div', { class: 'row center gap-8' },
        h('h2', { class: 'overlay-title', text: playlist.title }),
        h('span', { class: 'chip', text: block.format })),
      counter));

  const unsubscribe = subscribe('player', refresh);
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
  ], { wide: true, onClose: unsubscribe });
}
