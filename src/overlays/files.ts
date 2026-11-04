import { h } from '../dom';
import { icon, type IconName } from '../icons';
import { t } from '../i18n';
import { api, type FormatBlock, type Playlist, type Track } from '../api';
import { countBy, filesText, findBlock, groupByStatus, onDisk, statusText } from '../library';
import { cover } from '../components/cover';
import { liveBar, liveChip, patchLive } from '../components/live';
import { closeButton, openPanel } from '../components/overlay';
import { canPlayTrack, trackRow } from '../components/trackRow';
import { hueOf, playlistPicture } from '../covers';
import * as player from '../player';
import { isPlayable } from '../player';
import { ago } from '../relativeTime';
import { filterByTitle } from '../search';
import { subscribe, ui } from '../state';
import { canDownload, tools } from '../tools';
import { deviceLabel } from './devicePicker';

type Filter = 'all' | 'ok' | 'pending' | 'off';

const IN_FILTER: Record<Filter, (track: Track) => boolean> = {
  all: () => true,
  ok: track => track.status === 'ok',
  pending: track => track.status === 'pending' || track.status === 'failed',
  off: track => track.status === 'unavailable',
};

// The same mark on a file and on the filter that finds it. Not on disk yet is
// yellow, failed or not: both come down with the next sync.
const MARK: Record<Exclude<Filter, 'all'>, { icon: IconName; color: string }> = {
  ok: { icon: 'ok', color: 'mark-ok' },
  pending: { icon: 'pending', color: 'mark-warning' },
  off: { icon: 'close', color: 'mark-failed' },
};

function mark(filter: Exclude<Filter, 'all'>, name: IconName = MARK[filter].icon): HTMLElement {
  return h('span', { class: `state-mark ${MARK[filter].color}` }, icon(name, 15));
}

/** What goes on the right of a file: its mark, and when it arrived or why it is not there. */
function fileState(track: Track): HTMLElement {
  const text = statusText(track);
  if (track.status === 'ok') {
    const when = ago(track.addedDaysAgo);
    return h('span', { class: 'fa-state', title: text }, mark('ok'), h('span', { class: 'd', text: t(when.key, when.amount) }));
  }
  if (track.status === 'pending') return h('span', { class: 'fa-state' }, mark('pending'), h('span', { class: 'pill warn', text }));
  if (track.status === 'failed') return h('span', { class: 'fa-state', title: t('files.failedHint') }, mark('pending', 'warning'), h('span', { class: 'pill warn', text }));
  return h('span', { class: 'fa-state', title: text }, mark('off'), h('span', { class: 'pill off ellipsis', text }));
}

/**
 * "Ver archivos" of the Gestor: every file of one playlist+format and its
 * state. It follows the library while open, so with that playlist
 * downloading each file goes from «Pendiente» to its date as it lands, with a
 * short flash to see which one it was.
 */
export function openFilesOverlay(playlist: Playlist, opened: FormatBlock): void {
  let owner = playlist;
  let block = opened;
  let query = '';
  let filter: Filter = 'all';
  let known = new Set(onDisk(block.tracks).map(track => track.id));
  let landed = new Set<string>();

  const subtitle = h('div', { class: 'muted ellipsis' });
  const playAll = h('button', { class: 'round-play', onclick: () => {
    if (ui.nowPlaying?.blockKey === block.key) player.togglePlay();
    else player.playAll(block.key);
  } });
  const switcher = h('div', { class: 'segmented' });
  const chips = h('div', { class: 'toolbar' });
  const search = h('input', { class: 'input', attrs: { placeholder: t('songs.searchPlaceholder') } });
  search.addEventListener('input', () => {
    query = search.value;
    refreshList();
  });
  const notice = h('div', { class: 'fa-notice notice' });
  const list = h('div', { class: 'fa-list' });
  const foot = h('div', { class: 'fa-foot' });

  const playableHere = () => block.tracks.some(track => canPlayTrack(block, track));

  function refreshPlayButton(): void {
    const sounding = ui.nowPlaying?.blockKey === block.key && ui.playing;
    playAll.replaceChildren(icon(sounding ? 'pause' : 'play', 18, true));
    const label = t(sounding ? 'player.pause' : 'player.playAll');
    playAll.title = label;
    playAll.setAttribute('aria-label', label);
    playAll.disabled = !playableHere();
  }

  function refreshHead(): void {
    const missing = countBy(block.tracks, 'pending') + countBy(block.tracks, 'failed');
    subtitle.textContent = [
      block.format.toUpperCase(),
      filesText(block.tracks.length),
      missing ? t('files.missingShort', missing) : '',
    ].filter(Boolean).join(' · ');
    switcher.hidden = owner.formats.length < 2;
    switcher.replaceChildren(...owner.formats.map(other => h('button', {
      class: other.key === block.key ? 'on' : '',
      text: other.format.toUpperCase(),
      onclick: () => show(other),
    })));
    refreshPlayButton();
  }

  function refreshChips(): void {
    const counts: Record<Filter, number> = {
      all: block.tracks.length,
      ok: block.tracks.filter(IN_FILTER.ok).length,
      pending: block.tracks.filter(IN_FILTER.pending).length,
      off: block.tracks.filter(IN_FILTER.off).length,
    };
    // A filter left with nothing to show goes back to all of them.
    if (counts[filter] === 0) filter = 'all';
    const LABELS: Record<Filter, string> = {
      all: 'files.filter.all',
      ok: 'files.filter.ok',
      pending: 'files.filter.pending',
      off: 'files.filter.off',
    };
    const shown: Filter[] = ['all'];
    if (counts.ok > 0 && counts.ok < counts.all) shown.push('ok');
    if (counts.pending > 0) shown.push('pending');
    if (counts.off > 0) shown.push('off');
    chips.replaceChildren(...shown.map(key => h('button', {
      class: `fchip${filter === key ? ' on' : ''}`,
      onclick: () => {
        filter = key;
        refreshChips();
        refreshList();
      },
    }, key === 'all' ? null : mark(key), h('span', { text: t(LABELS[key]) }), h('small', { text: String(counts[key]) }))));
  }

  function refreshNotice(): void {
    if (block.accessible === false) {
      notice.replaceChildren(icon('warning', 16), h('span', {
        text: t('files.offline', block.device ? block.device.deviceName : owner.folder),
        title: block.device ? deviceLabel(block.device) : owner.folder,
      }));
      notice.hidden = false;
    } else if (!isPlayable(block.format)) {
      notice.replaceChildren(icon('warning', 16), h('span', { text: t('files.unplayable', block.format.toUpperCase()) }));
      notice.hidden = false;
    } else {
      notice.hidden = true;
    }
  }

  function refreshList(): void {
    const rows = groupByStatus(filterByTitle(block.tracks.filter(IN_FILTER[filter]), query));
    const scroll = list.scrollTop;
    list.replaceChildren(...(rows.length
      ? rows.map(track => trackRow(block, track, fileState(track), { landed: landed.has(track.id), thumbWidth: 64, thumbHeight: 38 }))
      : [h('p', { class: 'none', text: query.trim() ? t('player.search.none', query.trim()) : t('files.none') })]));
    list.scrollTop = scroll;
  }

  let footShape = '';
  const currentFootShape = () => {
    const task = ui.task;
    return `${block.key}|${task?.blockKey === block.key ? `live|${task.step}` : task ? 'busy' : 'idle'}|${canDownload(tools.report)}|${countBy(block.tracks, 'pending') + countBy(block.tracks, 'failed')}`;
  };

  function refreshFoot(): void {
    footShape = currentFootShape();
    const task = ui.task;
    if (task && task.blockKey === block.key) {
      foot.replaceChildren(
        liveChip(task, block.format),
        h('div', { class: 'grow' }, liveBar(task)),
        h('button', { class: 'secondary-button small-button danger-text', onclick: () => api.stopDownload() },
          icon('stopped', 14, true), h('span', { text: t('tasks.stop') })));
      foot.hidden = false;
      return;
    }
    const missing = countBy(block.tracks, 'pending') + countBy(block.tracks, 'failed');
    foot.hidden = missing === 0;
    if (missing === 0) return;
    const sync = h('button', { class: 'primary-button small-button', onclick: () => {
      if (canDownload(tools.report)) api.startDownload({ kind: 'update', blockKey: block.key });
    } }, icon('sync', 14), h('span', { text: t('manager.sync') }));
    sync.disabled = task !== null || !canDownload(tools.report) || block.accessible === false;
    foot.replaceChildren(
      h('span', { class: 'grow muted', text: missing === 1 ? t('files.missing.one') : t('files.missing.many', missing) }),
      sync);
  }

  function refreshAll(): void {
    refreshHead();
    refreshChips();
    refreshNotice();
    refreshList();
    refreshFoot();
  }

  function show(other: FormatBlock): void {
    block = other;
    known = new Set(onDisk(block.tracks).map(track => track.id));
    landed = new Set();
    filter = 'all';
    list.scrollTop = 0;
    refreshAll();
  }

  const reload = () => {
    const found = findBlock(block.key);
    if (found) {
      owner = found.playlist;
      block = found.block;
    }
    const now = onDisk(block.tracks).map(track => track.id);
    landed = new Set(now.filter(id => !known.has(id)));
    known = new Set(now);
    refreshAll();
    landed.clear();
  };

  // 'player' also fires several times a second with the position: the rows
  // (thumbnails included) are rebuilt only when what is playing changes.
  let shown = '';
  const playingKey = () => `${ui.nowPlaying?.blockKey}|${ui.nowPlaying?.track.id}|${ui.playing}`;
  const followPlayer = () => {
    if (playingKey() === shown) return;
    shown = playingKey();
    refreshPlayButton();
    refreshList();
  };
  const followTasks = () => {
    if (currentFootShape() === footShape) patchLive(foot);
    else refreshFoot();
  };
  const unsubscribe = [subscribe('player', followPlayer), subscribe('library', reload), subscribe('tasks', followTasks)];
  shown = playingKey();
  refreshAll();

  const head = h('div', { class: 'fa-head' },
    cover(playlist.title, block.video, 76, playlistPicture(playlist)),
    h('div', { class: 'grow min0' },
      h('div', { class: 'eyebrow', text: t(block.video ? 'player.kind.video' : 'player.kind.audio') }),
      h('h2', { class: 'ellipsis', text: playlist.title, title: playlist.title }),
      subtitle),
    h('div', { class: 'row', attrs: { style: 'align-self:flex-start' } }, closeButton()));
  head.style.setProperty('--h', String(hueOf(playlist.title)));

  openPanel(h('div', { class: 'mod wide' },
    head,
    h('div', { class: 'fa-tools' },
      playAll,
      switcher,
      chips,
      h('div', { class: 'search-box' }, icon('search', 15), search)),
    notice,
    list,
    foot), () => unsubscribe.forEach(stop => stop()));
}
