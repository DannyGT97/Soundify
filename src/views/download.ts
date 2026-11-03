import { card, h, stepHeading, titleRow } from '../dom';
import { t } from '../i18n';
import { destinationCard, formatSelect, summaryArt, summaryPanel, summaryRow } from '../components/form';
import { api } from '../api';
import { isPlaylistUrl, isWebUrl } from '../urls';
import { subscribeWhile } from '../state';
import { missingRequired, tools } from '../tools';
import { openToolsSetup } from '../overlays/toolsSetup';

/** Descarga por URL: one video at a time. A playlist URL is pointed to the Gestor. */
export function downloadView(): HTMLElement {
  const summaryUrl = h('div', { class: 'row-title break', text: t('download.summary.empty') });
  const summaryFormat = h('div', { class: 'row-title' });
  const summaryDestination = h('div', { class: 'row-title break', text: t('download.folder.default') });
  const summary = summaryPanel('download.summary.title', summaryArt(), [
    ...summaryRow('download.summary.url', summaryUrl, true),
    ...summaryRow('download.summary.format', summaryFormat),
    ...summaryRow('download.summary.destination', summaryDestination),
  ], 'download.summary.notice');

  const url = h('input', { class: 'input', attrs: { placeholder: 'https://www.youtube.com/watch?v=…' } });
  const playlistHint = h('p', { class: 'help error-text', text: t('download.isPlaylist') });
  playlistHint.hidden = true;
  let wasUsable = false;
  url.addEventListener('input', () => {
    const text = url.value.trim();
    const playlist = isPlaylistUrl(text);
    url.classList.toggle('input-error', playlist);
    playlistHint.hidden = !playlist;
    summaryUrl.textContent = text || t('download.summary.empty');
    // Opens by itself only when the field BECOMES usable: hiding it by hand is respected.
    const usable = isWebUrl(text) && !playlist;
    if (usable && !wasUsable) summary.open();
    wasUsable = usable;
  });

  const destination = destinationCard('02', 'download.folder.default', path => { summaryDestination.textContent = path; });
  const format = formatSelect();
  const syncFormat = () => { summaryFormat.textContent = format.value; };
  format.addEventListener('change', syncFormat);
  syncFormat();

  const start = h('button', { class: 'primary-button', text: t('download.start'), onclick: () => {
    if (!isWebUrl(url.value) || isPlaylistUrl(url.value)) {
      url.focus();
      url.classList.add('input-error');
      return;
    }
    const target = destination.device();
    api.startDownload(target
      ? { kind: 'single-to-device', url: url.value.trim(), format: format.value, target }
      : { kind: 'single', url: url.value.trim(), format: format.value, folder: destination.field.value });
  } });

  // Without yt-dlp or ffmpeg nothing downloads: said here, before the click,
  // and the button waits. Deno is only recommended and does not block.
  const missingText = h('span', { class: 'grow' });
  const missingNotice = h('div', { class: 'status-strip warning row center gap-10' }, missingText,
    h('button', { class: 'primary-button', text: t('download.toolsMissing.install'), onclick: openToolsSetup }));
  const showMissing = () => {
    const missing = missingRequired(tools.report);
    missingNotice.hidden = missing.length === 0;
    missingText.textContent = t('download.toolsMissing', missing.map(tool => tool.name).join(', '));
    start.disabled = missing.length > 0;
  };
  subscribeWhile(missingNotice, 'tools', showMissing);
  showMissing();

  return h('div', { class: 'screen-with-summary' },
    h('div', { class: 'screen' },
      h('div', { class: 'screen-head' },
        titleRow('download', t('download.title'), h('div', { class: 'grow' }), summary.showButton),
        h('p', { class: 'subtitle', text: t('download.description') })),
      missingNotice,
      card(stepHeading('01', t('download.url')), url, playlistHint, h('p', { class: 'help', text: t('download.source.hint') })),
      destination.card,
      card(stepHeading('03', t('download.format')), format, h('p', { class: 'help', text: t('download.format.hint') })),
      h('div', { class: 'row center gap-14' }, start, h('p', { class: 'help', text: t('download.caption') }))),
    summary.panel);
}
