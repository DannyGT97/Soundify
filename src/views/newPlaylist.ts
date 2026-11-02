import { card, h, stepHeading, titleRow } from '../dom';
import { t } from '../i18n';
import { destinationCard, formatSelect, isPlaylistUrl, summaryArt, summaryPanel, summaryRow } from '../components/form';
import { navigate } from '../router';
import { startTask } from '../tasks';
import { notify } from '../components/toasts';

/** Crear Playlist: registers a YouTube playlist in the Gestor (and optionally downloads it). */
export function newPlaylistView(): HTMLElement {
  const summaryUrl = h('div', { class: 'row-title break' });
  const summaryDestination = h('div', { class: 'row-title break', text: t('newPlaylist.summary.noFolder') });
  const summaryFormat = h('div', { class: 'row-title' });
  const summaryNumbering = h('div', { class: 'row-title', text: t('newPlaylist.notNumbered') });
  // PLACEHOLDER: «Cambiar portada…» will open the image dialog.
  const changeCover = h('button', { class: 'secondary-button full', text: t('newPlaylist.cover.change') });
  const summary = summaryPanel('newPlaylist.summary.title', summaryArt(), [
    changeCover,
    ...summaryRow('download.summary.url', summaryUrl, true),
    ...summaryRow('download.summary.destination', summaryDestination),
    ...summaryRow('download.summary.format', summaryFormat),
    ...summaryRow('newPlaylist.summary.numbering', summaryNumbering),
  ], 'newPlaylist.summary.notice');


  const url = h('input', { class: 'input', attrs: { placeholder: 'https://www.youtube.com/playlist?list=…' } });
  let wasUsable = false;
  url.addEventListener('input', () => {
    summaryUrl.textContent = url.value.trim();
    const usable = isPlaylistUrl(url.value);
    if (usable && !wasUsable) summary.open();
    wasUsable = usable;
  });

  const destination = destinationCard('02', 'download.folder.choose', path => { summaryDestination.textContent = path; });
  const format = formatSelect();
  const syncFormat = () => { summaryFormat.textContent = format.value; };
  format.addEventListener('change', syncFormat);
  syncFormat();

  const numbered = h('input', { attrs: { type: 'checkbox' } });
  numbered.addEventListener('change', () => {
    summaryNumbering.textContent = t(numbered.checked ? 'newPlaylist.numbered' : 'newPlaylist.notNumbered');
  });

  const submit = (download: boolean) => {
    if (!url.value.trim() || !destination.field.value) {
      notify({ tone: 'error', text: t('newPlaylist.missingFields') });
      return;
    }
    if (download) startTask(url.value.trim(), 'manager', 12);
    navigate('manager');
  };

  return h('div', { class: 'screen-with-summary' },
    h('div', { class: 'screen' },
      h('div', { class: 'screen-head' },
        titleRow('new-playlist', t('newPlaylist.title'), h('div', { class: 'grow' }), summary.showButton),
        h('p', { class: 'subtitle', text: t('newPlaylist.description') })),
      card(stepHeading('01', t('newPlaylist.url')), url),
      destination.card,
      card(stepHeading('03', t('download.format')), format),
      card(stepHeading('04', t('newPlaylist.numberFiles')),
        h('label', { class: 'check' }, numbered, h('span', { text: t('newPlaylist.numberFiles') })),
        h('p', { class: 'help', text: t('newPlaylist.numberFiles.desc') })),
      h('div', { class: 'row center gap-12' },
        h('button', { class: 'primary-button', text: t('newPlaylist.createAndDownload'), onclick: () => submit(true) }),
        h('button', { class: 'secondary-button', text: t('newPlaylist.create'), onclick: () => submit(false) }))),
    summary.panel);
}
