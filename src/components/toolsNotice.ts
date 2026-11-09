import { h } from '../dom';
import { t } from '../i18n';
import { openToolsSetup } from '../overlays/toolsSetup';
import { onAndroid } from '../platform';
import { subscribeWhile } from '../state';
import { grantStorage, needsStorageAccess } from '../storage';
import { missingRequired, tools } from '../tools';

/**
 * The yellow notice of the screens that download (Descarga por URL, Crear
 * Playlist, Gestor): without yt-dlp or ffmpeg nothing downloads, and it is said
 * before the click. Hidden while nothing required is missing (Deno is only
 * recommended and does not block).
 *
 * On the phone the tools travel in the APK, so there is nothing to install:
 * the notice says the storage access is missing (with the button that asks
 * for it), that the tools are still being unpacked, or that they could not be.
 */
export function toolsNotice(): HTMLElement {
  const text = h('span', { class: 'grow' });
  const button = h('button', { class: 'primary-button' });
  const notice = h('div', { class: 'status-strip warning row center gap-10' }, text, button);
  const show = () => {
    const missing = missingRequired(tools.report);
    if (onAndroid) {
      const noAccess = needsStorageAccess();
      notice.hidden = !noAccess && missing.length === 0;
      text.textContent = t(noAccess ? 'storage.notice' : tools.unpackFailed ? 'tools.android.failed' : 'tools.android.preparing');
      button.hidden = !noAccess;
      button.textContent = t('storage.grant');
      button.onclick = () => void grantStorage();
      return;
    }
    notice.hidden = missing.length === 0;
    text.textContent = t('tools.missing.notice', missing.map(tool => tool.name).join(', '));
    button.textContent = t('tools.missing.install');
    button.onclick = openToolsSetup;
  };
  subscribeWhile(notice, 'tools', show);
  subscribeWhile(notice, 'storage', show);
  show();
  return notice;
}
