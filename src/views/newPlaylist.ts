import { h, titleRow } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { destinations, fieldLabel, footerSummary, formatPicker, urlCover } from '../components/form';
import { api } from '../api';
import { navigate, type Params } from '../router';
import { isWebUrl } from '../urls';
import { notify } from '../components/toasts';
import { canCreateFrom, createdNotice } from '../createPlaylist';
import { findBlock, loadLibrary } from '../library';
import { errorText } from '../transfer';
import { subscribeWhile } from '../state';
import { canDownload, tools } from '../tools';
import { toolsNotice } from '../components/toolsNotice';

/** Crear Playlist: registers a YouTube playlist in the Gestor (and optionally downloads it). */
export function newPlaylistView(params: Params = {}): HTMLElement {
  const url = h('input', { class: 'input url-input', attrs: { id: 'url', placeholder: 'https://www.youtube.com/playlist?list=…' } });
  url.value = params.url ?? '';
  const hint = h('div', { class: 'hint' });
  let busy = false;

  const numbered = h('input', { class: 'switch', attrs: { type: 'checkbox' } });
  const sample = h('span', { class: 'sample' });

  const sync = () => {
    const text = url.value.trim();
    const web = isWebUrl(text);
    const listed = web && canCreateFrom(text);
    picture.look(listed ? text : null);

    if (text && !web) hint.replaceChildren(h('span', { class: 'error-text', text: t('form.hint.notUrl') }));
    else if (web && !listed) {
      hint.replaceChildren(h('span', { class: 'error-text', text: t('newPlaylist.hint.notPlaylist') }),
        h('button', { class: 'link-button', text: t('newPlaylist.hint.toDownload'), onclick: () => navigate('download', { url: text }) }));
    } else if (listed) {
      hint.replaceChildren(h('span', { text: t(picture.loading() ? 'newPlaylist.hint.querying'
        : picture.picked() ? 'newPlaylist.hint.picked' : 'newPlaylist.hint.auto') }));
    } else hint.replaceChildren(h('span', { text: t('newPlaylist.hint.empty') }));

    sample.textContent = `${numbered.checked ? '001 - ' : ''}Neon Rain.${format.value().toLowerCase()}`;

    const where = destination.value();
    const blocked = !canDownload(tools.report);
    const off = blocked || busy || !listed || !where;
    create.disabled = off;
    createAndDownload.disabled = off;
    summary.show(where ? `${format.value()} · ${where}` : format.value(),
      busy ? t('newPlaylist.querying')
        : blocked ? t('form.foot.tools')
        : !text ? t('form.foot.noUrl')
        : !where ? t('form.foot.noDest')
        : t('newPlaylist.foot.ready'));
  };

  const picture = urlCover(true, 'form.cover.cover', () => sync());
  const format = formatPicker(() => sync());
  const destination = destinations(() => sync());
  const summary = footerSummary();

  // Registered first, ALWAYS (the Java order): the playlist is in the Gestor
  // the moment the screen gets there, and «Crear y descargar» then runs the
  // same update as the Gestor's «Actualizar».
  const submit = async (download: boolean) => {
    if (!canDownload(tools.report)) return;
    const link = url.value.trim();
    if (!link || !destination.value()) {
      notify({ tone: 'error', text: t('newPlaylist.missingFields') });
      return;
    }
    if (!canCreateFrom(link)) {
      notify({ tone: 'error', text: t('newPlaylist.notAPlaylist') });
      return;
    }
    busy = true;
    sync();
    try {
      const target = destination.device();
      const created = target
        ? await api.createDevicePlaylist(link, format.value(), target, numbered.checked)
        : await api.createPlaylist(link, format.value(), destination.value(), numbered.checked);
      const notice = createdNotice(created);
      const pickedCover = picture.picked();
      if (pickedCover) {
        try {
          await api.setCover(created.blockKey, pickedCover);
        } catch {
          // The playlist is there all the same: said, not undone.
          notice.tone = 'warning';
          notice.text = `${notice.text} ${t('newPlaylist.cover.failed')}`;
        }
      }
      notify(notice);
      await loadLibrary();
      // The playlist's own picture, unless it already has one (another format
      // of it, whose cover was chosen by hand). Without it, its first video
      // shows anyway, so a failure here is nothing to bother anyone with.
      const foundCover = picture.found();
      if (!pickedCover && foundCover && !findBlock(created.blockKey)?.playlist.cover) {
        await api.setCover(created.blockKey, foundCover).then(loadLibrary).catch(() => {});
      }
      if (download) api.startDownload({ kind: 'update', blockKey: created.blockKey });
      navigate('manager');
    } catch (error) {
      notify({ tone: 'error', text: errorText(error) });
    } finally {
      busy = false;
      sync();
    }
  };

  const create = h('button', { class: 'secondary-button', text: t('newPlaylist.create'), onclick: () => void submit(false) });
  const createAndDownload = h('button', { class: 'primary-button', onclick: () => void submit(true) },
    icon('arrow-down', 16), h('span', { text: t('newPlaylist.createAndDownload') }));

  url.addEventListener('input', sync);
  numbered.addEventListener('change', sync);
  // Without yt-dlp or ffmpeg nothing is created either: a playlist is registered by asking yt-dlp.
  subscribeWhile(create, 'tools', sync);
  sync();

  return h('div', { class: 'screen fp' },
    h('div', { class: 'fp-body' },
      h('div', { class: 'screen-head' }, titleRow('new-playlist', t('newPlaylist.title'))),
      toolsNotice(),
      h('div', { class: 'url-wrap' }, picture.node,
        h('div', { class: 'grow min0' }, h('label', { class: 'field-label', text: t('newPlaylist.url'), attrs: { for: 'url' } }), url, hint)),
      h('div', {}, fieldLabel('download.format'), format.node),
      h('div', {}, fieldLabel('download.saveIn'), destination.node),
      h('label', { class: 'switch-row' }, numbered,
        h('div', { class: 'min0' }, h('b', { text: t('newPlaylist.numberFiles') }), h('small', { text: t('newPlaylist.numberFiles.short') })),
        sample)),
    h('div', { class: 'fp-foot' }, summary.node, create, createAndDownload));
}
