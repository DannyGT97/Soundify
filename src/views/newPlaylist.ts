import { card, h, stepHeading, titleRow } from '../dom';
import { t } from '../i18n';
import { destinationCard, formatSelect, summaryArt, summaryPanel, summaryRow } from '../components/form';
import { api } from '../api';
import { navigate } from '../router';
import { isPlaylistUrl } from '../urls';
import { notify } from '../components/toasts';
import { canCreateFrom, createdNotice } from '../createPlaylist';
import { loadLibrary } from '../library';
import { errorText } from '../transfer';
import { showPicture } from '../covers';

/** Crear Playlist: registers a YouTube playlist in the Gestor (and optionally downloads it). */
export function newPlaylistView(): HTMLElement {
  const summaryUrl = h('div', { class: 'row-title break' });
  const summaryDestination = h('div', { class: 'row-title break', text: t('newPlaylist.summary.noFolder') });
  const summaryFormat = h('div', { class: 'row-title' });
  const summaryNumbering = h('div', { class: 'row-title', text: t('newPlaylist.notNumbered') });
  // The cover: the one picked here, else the URL's first video. The picked
  // one is only copied in once the playlist exists (there is no folder yet).
  const art = summaryArt();
  let pendingCover: string | null = null;
  let urlVideoId: string | null = null;
  const showCover = () => showPicture(art, pendingCover ? { kind: 'file', path: pendingCover }
      : urlVideoId ? { kind: 'thumbnail', id: urlVideoId } : null);
  const changeCover = h('button', { class: 'secondary-button full', text: t('newPlaylist.cover.change'), onclick: async () => {
    const image = await api.pickCoverImage(t('newPlaylist.cover.title'), t('cover.choose.filter'));
    if (!image) return;
    pendingCover = image;
    showCover();
  } });
  const summary = summaryPanel('newPlaylist.summary.title', art, [
    changeCover,
    ...summaryRow('download.summary.url', summaryUrl, true),
    ...summaryRow('download.summary.destination', summaryDestination),
    ...summaryRow('download.summary.format', summaryFormat),
    ...summaryRow('newPlaylist.summary.numbering', summaryNumbering),
  ], 'newPlaylist.summary.notice');


  const url = h('input', { class: 'input', attrs: { placeholder: 'https://www.youtube.com/playlist?list=…' } });
  let wasUsable = false;
  let coverTimer: number | undefined;
  url.addEventListener('input', () => {
    summaryUrl.textContent = url.value.trim();
    const usable = isPlaylistUrl(url.value);
    if (usable && !wasUsable) summary.open();
    wasUsable = usable;
    // Asked once typing stops (the query may go to YouTube), and an answer
    // for a URL that is no longer in the field is dropped.
    window.clearTimeout(coverTimer);
    urlVideoId = null;
    showCover();
    if (!usable) return;
    const asked = url.value.trim();
    coverTimer = window.setTimeout(() => {
      void api.firstVideoId(asked).catch(() => null).then(id => {
        if (url.value.trim() !== asked) return;
        urlVideoId = id;
        showCover();
      });
    }, 700);
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

  // Registered first, ALWAYS (the Java order): the playlist is in the Gestor
  // the moment the screen gets there, and «Crear y descargar» then runs the
  // same update as the Gestor's «Actualizar».
  const submit = async (download: boolean) => {
    const link = url.value.trim();
    if (!link || !destination.field.value) {
      notify({ tone: 'error', text: t('newPlaylist.missingFields') });
      return;
    }
    if (!canCreateFrom(link)) {
      notify({ tone: 'error', text: t('newPlaylist.notAPlaylist') });
      return;
    }
    setBusy(true);
    try {
      const target = destination.device();
      const created = target
        ? await api.createDevicePlaylist(link, format.value, target, numbered.checked)
        : await api.createPlaylist(link, format.value, destination.field.value, numbered.checked);
      const notice = createdNotice(created);
      if (pendingCover) {
        try {
          await api.setCover(created.blockKey, pendingCover);
        } catch {
          // The playlist is there all the same: said, not undone.
          notice.tone = 'warning';
          notice.text = `${notice.text} ${t('newPlaylist.cover.failed')}`;
        }
      }
      notify(notice);
      await loadLibrary();
      if (download) api.startDownload({ kind: 'update', blockKey: created.blockKey });
      navigate('manager');
    } catch (error) {
      notify({ tone: 'error', text: errorText(error) });
    } finally {
      setBusy(false);
    }
  };

  const createAndDownload = h('button', { class: 'primary-button', text: t('newPlaylist.createAndDownload'), onclick: () => void submit(true) });
  const create = h('button', { class: 'secondary-button', text: t('newPlaylist.create'), onclick: () => void submit(false) });
  // Asking YouTube takes a few seconds: said, and not asked twice meanwhile.
  const querying = h('p', { class: 'help', text: t('newPlaylist.querying') });
  querying.hidden = true;
  function setBusy(busy: boolean): void {
    createAndDownload.disabled = busy;
    create.disabled = busy;
    querying.hidden = !busy;
  }

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
      h('div', { class: 'row center gap-12' }, createAndDownload, create),
      querying),
    summary.panel);
}
