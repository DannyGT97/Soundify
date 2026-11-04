import { h } from '../dom';
import { t } from '../i18n';
import { openToolsSetup } from '../overlays/toolsSetup';
import { subscribeWhile } from '../state';
import { missingRequired, tools } from '../tools';

/**
 * The yellow notice of the screens that download (Descarga por URL, Crear
 * Playlist, Gestor): without yt-dlp or ffmpeg nothing downloads, and it is said
 * before the click. Hidden while nothing required is missing (Deno is only
 * recommended and does not block).
 */
export function toolsNotice(): HTMLElement {
  const text = h('span', { class: 'grow' });
  const notice = h('div', { class: 'status-strip warning row center gap-10' }, text,
    h('button', { class: 'primary-button', text: t('tools.missing.install'), onclick: openToolsSetup }));
  const show = () => {
    const missing = missingRequired(tools.report);
    notice.hidden = missing.length === 0;
    text.textContent = t('tools.missing.notice', missing.map(tool => tool.name).join(', '));
  };
  subscribeWhile(notice, 'tools', show);
  show();
  return notice;
}
