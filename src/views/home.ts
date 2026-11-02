import { h, titleRow } from '../dom';
import { t } from '../i18n';
import { countBy, onDisk, playlists } from '../mock';
import { navigate } from '../router';
import { cover } from '../components/cover';

/** Inicio: the whole library at a glance and the playlist list, which leads to the Gestor. */
export function homeView(): HTMLElement {
  const all = playlists.flatMap(p => p.formats.flatMap(b => b.tracks));
  const files = onDisk(all).length;
  const pending = countBy(all, 'pending') + countBy(all, 'failed');
  const unavailable = countBy(all, 'unavailable');
  const total = Math.max(1, all.length);

  const bar = h('div', { class: 'stats-bar' },
    segment('ok', files / total), segment('warn', pending / total), segment('error', unavailable / total));

  const rows = playlists.map(p => {
    const tracks = p.formats.flatMap(b => b.tracks);
    const waiting = countBy(tracks, 'pending');
    return h('button', { class: 'list-row', onclick: () => navigate('manager', { open: p.id }) },
      cover(p.title, p.formats.every(b => b.video), 40),
      h('div', { class: 'grow min0' },
        h('div', { class: 'row-title ellipsis', text: p.title }),
        h('div', { class: 'subtitle ellipsis', text: p.folder })),
      h('div', { class: 'list-row-meta' },
        h('div', { class: 'row-title', text: t('home.meta.files', onDisk(tracks).length) }),
        waiting ? h('div', { class: 'warn-text', text: t('home.meta.pending', waiting) }) : null,
        h('div', { class: 'subtitle', text: p.formats.map(b => b.format).join(' · ') })));
  });

  return h('div', { class: 'screen' },
    h('div', { class: 'screen-head' },
      titleRow('home', t('home.title'), h('div', { class: 'grow' }),
        h('span', { class: 'chip', text: t('app.version') })),
      h('p', { class: 'subtitle', text: t('app.description') })),
    h('section', { class: 'card' }, bar,
      h('div', { class: 'row gap-10' },
        stat(files, 'home.stat.files', 'ok'), stat(pending, 'home.stat.pending', 'warn'),
        stat(unavailable, 'home.stat.unavailable', 'error'))),
    h('section', { class: 'card' },
      h('div', { class: 'row center gap-10' },
        h('div', { class: 'card-title', text: t('home.yourPlaylists', playlists.length) }),
        h('div', { class: 'grow' }),
        h('button', { class: 'secondary-button', text: t('home.seeAll'), onclick: () => navigate('manager') })),
      h('div', { class: 'stack-6' }, ...rows)));
}

function segment(kind: string, share: number): HTMLElement {
  const el = h('div', { class: `stats-${kind}` });
  el.style.flexGrow = String(share);
  return el;
}

function stat(value: number, captionKey: string, kind: string): HTMLElement {
  return h('div', { class: 'stat-chip' },
    h('div', { class: `stat-value stat-${kind}`, text: String(value) }),
    h('div', { class: 'stat-caption', text: t(captionKey) }));
}
