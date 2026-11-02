import { h, titleRow } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import type { FormatBlock, Playlist, Track } from '../api';
import { findBlock, library, onDisk } from '../library';
import * as player from '../player';
import { PLAYABLE } from '../player';
import { navigate, onLeave, type Params } from '../router';
import { subscribe, ui } from '../state';
import { cover } from '../components/cover';

/**
 * Reproductor: the local playlists on the left (one row per playlist+format)
 * and the chosen one on the right. Clicking a file plays it and goes to
 * Reproduciendo; playback goes on when leaving, the controls live in the bar.
 */
export function playerView(params: Params): HTMLElement {
  ui.lastPlayerPage = 'player';
  if (params.b) ui.selectedBlock = params.b;

  const left = h('div', { class: 'stack-4 scroll' });
  const right = h('section', { class: 'card player-detail' });
  const render = () => {
    left.replaceChildren(...library.playlists.flatMap(p => p.formats.map(b => playlistRow(p, b))));
    right.replaceChildren(...detail());
  };
  render();
  onLeave(subscribe('player', () => markPlaying(right)));

  return h('div', { class: 'screen' },
    h('div', { class: 'screen-head' },
      titleRow('player', t('player.title')),
      h('p', { class: 'subtitle', text: t('player.description') })),
    h('div', { class: 'player-columns' },
      h('aside', { class: 'panel player-list' }, h('div', { class: 'summary-title', text: t('player.playlists') }), left),
      right));
}

function playlistRow(playlist: Playlist, block: FormatBlock): HTMLElement {
  const playable = PLAYABLE.has(block.format);
  const row = h('button', { class: `player-playlist${ui.selectedBlock === block.key ? ' selected' : ''}${playable ? '' : ' off'}`,
    title: playable ? '' : t('player.unplayable', block.format),
    onclick: () => navigate('player', { b: block.key }) },
    cover(playlist.title, block.video, 40),
    h('div', { class: 'min0' },
      h('div', { class: 'row-title ellipsis', text: playlist.title }),
      h('div', { class: 'subtitle', text: t('player.meta', block.format, onDisk(block.tracks).length) })));
  return row;
}

function detail(): Node[] {
  const found = findBlock(ui.selectedBlock);
  if (!found) return [h('p', { class: 'help', text: t('player.pick') })];
  const { playlist, block } = found;
  const playable = PLAYABLE.has(block.format);
  const tracks = onDisk(block.tracks);

  const list = h('div', { class: 'track-list' });
  const noMatch = h('p', { class: 'help' });
  const fill = (query: string) => {
    const shown = tracks.filter(tr => tr.title.toLowerCase().includes(query.toLowerCase()));
    list.replaceChildren(...shown.map((tr, i) => trackRow(block, tr, i + 1, playable)));
    noMatch.hidden = shown.length > 0;
    noMatch.textContent = t('player.search.none', query);
  };

  const search = h('input', { class: 'input search-field', attrs: { placeholder: t('player.search.prompt') } });
  search.hidden = true;
  search.addEventListener('input', () => fill(search.value));
  const searchButton = h('button', { class: 'search-button', title: t('player.search'), onclick: () => {
    search.hidden = !search.hidden;
    if (!search.hidden) search.focus();
    else { search.value = ''; fill(''); }
  } });
  searchButton.append(icon('search'));
  const playAll = h('button', { class: 'play-big', title: t('player.play'), onclick: () => {
    player.playAll(block.key);
    navigate('now-playing');
  } }, icon('play', 20, true));
  playAll.disabled = !playable;

  fill('');
  return [
    h('div', { class: 'row center gap-18' },
      cover(playlist.title, block.video, 96),
      h('div', { class: 'grow min0' },
        h('h2', { class: 'page-title ellipsis', text: playlist.title }),
        h('div', { class: 'subtitle', text: t('player.meta', block.format, tracks.length) }),
        playable ? null : h('div', { class: 'warn-text', text: t('player.unplayable', block.format) })),
      search, searchButton, playAll),
    h('div', { class: 'track-columns' },
      h('span', { class: 'col-num', text: '#' }), h('span', { class: 'col-cover' }),
      h('span', { class: 'grow', text: t('player.column.title') }),
      h('span', { class: 'col-added', text: t('player.column.added') })),
    noMatch,
    list,
  ];
}

function trackRow(block: FormatBlock, track: Track, index: number, playable: boolean): HTMLElement {
  const playing = ui.nowPlaying?.track.id === track.id;
  return h('button', { class: `track-row${playing ? ' playing' : ''}${playable ? '' : ' off'}`, attrs: { 'data-id': track.id },
    onclick: () => {
      if (!playable) return;
      player.play(block.key, track);
      navigate('now-playing');
    } },
    h('span', { class: 'col-num', text: String(index) }),
    cover(track.title, block.video, 40),
    h('span', { class: 'grow ellipsis row-title', text: track.title }),
    h('span', { class: 'col-added subtitle', text: relative(track.addedDaysAgo) }));
}

/** Keeps the green "now playing" mark on the right row without rebuilding the list. */
function markPlaying(root: HTMLElement): void {
  root.querySelectorAll<HTMLElement>('.track-row').forEach(row => {
    row.classList.toggle('playing', row.dataset.id === ui.nowPlaying?.track.id);
  });
}

function relative(days: number): string {
  if (days === 0) return t('relative.today');
  if (days === 1) return t('relative.yesterday');
  if (days < 7) return t('relative.days', days);
  if (days < 30) return t('relative.weeks', Math.floor(days / 7));
  return t('relative.months', Math.floor(days / 30));
}
