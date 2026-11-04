import { h, titleRow } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import type { Playlist } from '../api';
import { countBy, filesText, findBlock, library, onDisk, pendingText } from '../library';
import { navigate, onLeave } from '../router';
import { subscribe, ui } from '../state';
import * as player from '../player';
import { cover } from '../components/cover';
import { liveBar, liveChip, patchLive } from '../components/live';
import { hueOf, playlistPicture } from '../covers';
import { openImportHelp } from '../overlays/help';
import { toolsNotice } from '../components/toolsNotice';

/** Inicio: what sounds, how the library is doing, and the playlists. */
export function homeView(): HTMLElement {
  const content = h('div', { class: 'stack-14' });
  let liveKey = '';
  let playKey = '';
  const currentLive = () => (ui.task ? `${ui.task.blockKey ?? ''}|${ui.task.step}` : '');
  const currentPlay = () => `${ui.nowPlaying?.blockKey}|${ui.nowPlaying?.track.id}|${ui.playing}`;
  const render = () => {
    liveKey = currentLive();
    playKey = currentPlay();
    if (!library.playlists.length) {
      content.replaceChildren(emptyHome());
      return;
    }
    content.replaceChildren(...[resumeCard(), healthCard(), quickRow(), playlistsSection()].filter((node): node is HTMLElement => node !== null));
  };
  onLeave(subscribe('library', render));
  onLeave(subscribe('tasks', () => currentLive() === liveKey ? patchLive(content) : render()));
  onLeave(subscribe('player', () => { if (currentPlay() !== playKey) render(); }));
  render();

  return h('div', { class: 'screen home' },
    titleRow('home', t('home.title')),
    toolsNotice(),
    content);
}

function emptyHome(): HTMLElement {
  return h('div', { class: 'empty' },
    h('div', { class: 'empty-mark' }, icon('music', 32)),
    h('h2', { text: t('home.empty.title') }),
    h('p', { text: t('home.empty.body') }),
    h('div', { class: 'row gap-8' },
      h('button', { class: 'primary-button', onclick: () => navigate('new-playlist') }, icon('new-playlist', 16), h('span', { text: t('nav.newPlaylist') })),
      h('button', { class: 'secondary-button', onclick: () => navigate('download') }, icon('download', 16), h('span', { text: t('nav.download') })),
      h('button', { class: 'secondary-button desktop-only', onclick: openImportHelp }, icon('import', 16), h('span', { text: t('manager.import') }))));
}

/** «Sigue donde lo dejaste»: pressing it plays or pauses, without leaving Inicio. */
function resumeCard(): HTMLElement | null {
  const now = ui.nowPlaying;
  const found = now ? findBlock(now.blockKey) : null;
  if (!now || !found) return null;
  const card = h('div', { class: 'resume', attrs: { role: 'button', tabindex: '0' }, onclick: () => player.togglePlay() },
    cover(now.track.title, found.block.video, 56, { kind: 'thumbnail', id: now.track.id }),
    h('div', { class: 'grow min0' },
      h('div', { class: 'eyebrow', text: t(ui.playing ? 'home.resume.playing' : 'home.resume.last') }),
      h('b', { class: 'ellipsis', text: now.track.title }),
      h('div', { class: 'muted ellipsis', text: `${found.playlist.title} · ${found.block.format.toUpperCase()}` })),
    h('span', { class: 'round-play' }, icon(ui.playing ? 'pause' : 'play', 18, true)));
  card.style.setProperty('--h', String(hueOf(found.playlist.title)));
  card.addEventListener('keydown', event => {
    if (event.key === 'Enter') { event.preventDefault(); player.togglePlay(); }
  });
  return card;
}

function healthCard(): HTMLElement {
  const blocks = library.playlists.flatMap(playlist => playlist.formats);
  const sum = (count: (tracks: typeof blocks[number]['tracks']) => number) => blocks.reduce((n, block) => n + count(block.tracks), 0);
  const files = sum(tracks => countBy(tracks, 'ok'));
  const pending = sum(tracks => countBy(tracks, 'pending') + countBy(tracks, 'failed'));
  const off = sum(tracks => countBy(tracks, 'unavailable'));
  const part = (n: number, color: string) => {
    if (!n) return null;
    const piece = h('i');
    piece.style.flex = String(n);
    piece.style.background = `var(${color})`;
    return piece;
  };
  const legend = (n: number, color: string, text: string) => {
    const dot = h('i', { class: 'dot' });
    dot.style.background = `var(${color})`;
    return h('span', {}, dot, h('b', { text: String(n) }), h('span', { text }));
  };
  const task = ui.task;
  const running = task?.blockKey ? findBlock(task.blockKey) : null;
  return h('div', { class: 'health' },
    h('div', { class: 'health-bar', attrs: { role: 'img', 'aria-label': t('home.health.summary', files, pending, off) } },
      part(files, '--sp-success'), part(pending, '--sp-warn'), part(off, '--sp-error')),
    h('div', { class: 'legend' },
      legend(files, '--sp-success', t('home.health.files')),
      legend(pending, '--sp-warn', t('home.health.pending')),
      legend(off, '--sp-error', t('home.health.off')),
      pending && !task ? h('button', { class: 'link-button', text: t('home.seePending'), onclick: () => navigate('manager', { filter: 'pending' }) }) : null),
    task ? h('div', { class: 'row center gap-8 min0' },
      liveChip(task, running?.block.format ?? ''),
      h('span', { class: 'ellipsis muted grow', text: running?.playlist.title ?? task.title }),
      h('button', { class: 'link-button', text: t('tasks.goTo'), onclick: () => {
        if (running) navigate('manager', { open: running.playlist.id, focus: running.block.key });
        else navigate('download');
      } })) : null,
    task ? liveBar(task) : null);
}

/** Only these two: Descarga por URL has its place in the menu. */
function quickRow(): HTMLElement {
  return h('div', { class: 'quick' },
    h('button', { onclick: () => navigate('new-playlist') }, icon('new-playlist', 18), h('span', { class: 'ellipsis', text: t('nav.newPlaylist') })),
    h('button', { class: 'desktop-only', onclick: openImportHelp }, icon('import', 18), h('span', { class: 'ellipsis', text: t('manager.import') })));
}

function playlistsSection(): HTMLElement {
  return h('div', {},
    h('div', { class: 'section-head' },
      h('b', { text: t('home.playlists') }),
      h('span', { class: 'muted', text: String(library.playlists.length) }),
      h('button', { class: 'link-button', text: t('home.seeAll'), onclick: () => navigate('manager') })),
    h('div', { class: 'pgrid small' }, ...library.playlists.map(playlistCard)));
}

function playlistCard(playlist: Playlist): HTMLElement {
  const files = playlist.formats.reduce((n, block) => n + onDisk(block.tracks).length, 0);
  const pending = playlist.formats.reduce((n, block) => n + countBy(block.tracks, 'pending') + countBy(block.tracks, 'failed'), 0);
  const device = playlist.formats.find(block => block.device)?.device;
  const first = playlist.formats[0];
  return h('button', { class: 'pcard', title: playlist.title, onclick: () => {
    if (first) navigate('player', { b: first.key });
  } },
    cover(playlist.title, playlist.formats.every(block => block.video), 120, playlistPicture(playlist)),
    device ? h('span', { class: 'tag-on' }, icon('device', 11), h('span', { text: device.deviceName })) : null,
    h('b', { class: 'ellipsis', text: playlist.title }),
    h('small', { class: pending ? 'warn-text' : '', text: pending ? pendingText(pending) : filesText(files) }));
}
