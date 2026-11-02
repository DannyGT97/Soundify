import { h } from '../dom';
import { t } from '../i18n';
import { api } from '../api';
import { openOverlay } from '../components/overlay';

/** "Actividad": the raw yt-dlp output of the download in progress. */
export function openActivityOverlay(): void {
  const log = h('pre', { class: 'log-area' });
  void api.activityLog().then(lines => { log.textContent = lines.join('\n'); });
  openOverlay(t('activity.title'), [
    log,
    h('div', { class: 'row end' },
      h('button', { class: 'secondary-button', text: t('activity.clear'), onclick: () => { log.textContent = ''; } })),
  ], { wide: true });
}
