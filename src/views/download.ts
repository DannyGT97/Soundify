import { h, titleRow } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { destinations, fieldLabel, footerSummary, formatPicker, keepsPickedCover, urlCover } from '../components/form';
import { api } from '../api';
import { navigate, type Params } from '../router';
import { isPlaylistUrl, isWebUrl } from '../urls';
import { subscribeWhile } from '../state';
import { canDownload, tools } from '../tools';
import { toolsNotice } from '../components/toolsNotice';

/** Descarga por URL: one video at a time. A URL that names a list is refused and pointed to Crear Playlist. */
export function downloadView(params: Params = {}): HTMLElement {
  const url = h('input', { class: 'input url-input', attrs: { id: 'url', placeholder: t('download.url.placeholder') } });
  url.value = params.url ?? '';
  const hint = h('div', { class: 'hint' });
  const formatWarning = h('div', { class: 'hint warn-text' });

  const sync = () => {
    const text = url.value.trim();
    const web = isWebUrl(text);
    const playlist = isPlaylistUrl(text);
    const single = web && !playlist;
    picture.look(single ? text : null);
    url.classList.toggle('input-error', playlist);

    if (text && !web) hint.replaceChildren(h('span', { class: 'error-text', text: t('form.hint.notUrl') }));
    else if (playlist) {
      hint.replaceChildren(h('span', { class: 'warn-text', text: t('download.hint.isPlaylist') }),
        h('button', { class: 'link-button', text: t('download.hint.toPlaylist'), onclick: () => navigate('new-playlist', { url: text }) }));
    } else if (single) hint.replaceChildren(h('span', { text: t(picture.loading() ? 'download.hint.querying' : 'download.hint.single') }));
    else hint.replaceChildren(h('span', { text: t('download.hint.empty') }));

    const lostPicture = !!picture.picked() && !keepsPickedCover(format.value());
    formatWarning.hidden = !lostPicture;
    formatWarning.textContent = lostPicture ? t('download.cover.notKept', format.value()) : '';

    const where = destination.value();
    const blocked = !canDownload(tools.report);
    start.disabled = blocked || !single || !where;
    summary.show(where ? `${format.value()} · ${where}` : format.value(),
      blocked ? t('form.foot.tools')
        : !web ? t('form.foot.noUrl')
        : playlist ? t('download.foot.isPlaylist')
        : !where ? t('form.foot.noDest')
        : t('download.foot.ready'));
  };

  const picture = urlCover(false, 'form.cover.thumbnail', () => sync());
  const format = formatPicker(() => sync());
  const destination = destinations(() => sync());
  const summary = footerSummary();

  const start = h('button', { class: 'primary-button', onclick: () => {
    if (!canDownload(tools.report)) return;
    if (!isWebUrl(url.value) || isPlaylistUrl(url.value)) {
      url.focus();
      url.classList.add('input-error');
      return;
    }
    const target = destination.device();
    const cover = picture.picked();
    api.startDownload(target
      ? { kind: 'single-to-device', url: url.value.trim(), format: format.value(), target, cover }
      : { kind: 'single', url: url.value.trim(), format: format.value(), folder: destination.value(), cover });
  } }, icon('arrow-down', 16), h('span', { text: t('download.start') }));

  url.addEventListener('input', sync);
  // Without yt-dlp or ffmpeg the button waits, and the notice says why.
  subscribeWhile(start, 'tools', sync);
  sync();

  return h('div', { class: 'screen fp' },
    h('div', { class: 'fp-body' },
      h('div', { class: 'screen-head' }, titleRow('download', t('download.title'))),
      toolsNotice(),
      h('div', { class: 'url-wrap' }, picture.node,
        h('div', { class: 'grow min0' }, h('label', { class: 'field-label', text: t('download.url'), attrs: { for: 'url' } }), url, hint)),
      h('div', {}, fieldLabel('download.format'), format.node, formatWarning),
      h('div', {}, fieldLabel('download.saveIn'), destination.node)),
    h('div', { class: 'fp-foot' }, summary.node, start));
}
