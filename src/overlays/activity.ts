import { h } from '../dom';
import { t } from '../i18n';
import { api } from '../api';
import { openOverlay } from '../components/overlay';
import { subscribe, ui } from '../state';
import { activityText } from '../tasks';

/** "Actividad": what the download in progress is doing, live. */
export function openActivityOverlay(): void {
  const log = h('pre', { class: 'log-area' });
  const refresh = () => {
    // Following the end only if the user was already there: scrolling up to
    // read a line must not be undone by the next one.
    const atEnd = log.scrollTop + log.clientHeight >= log.scrollHeight - 4;
    log.textContent = ui.activity.map(activityText).join('\n');
    if (atEnd) log.scrollTop = log.scrollHeight;
  };
  const unsubscribe = subscribe('activity', refresh);
  refresh();
  openOverlay(t('activity.title'), [
    log,
    h('div', { class: 'row end' },
      h('button', { class: 'secondary-button', text: t('activity.clear'), onclick: api.clearActivity })),
  ], { wide: true, onClose: unsubscribe });
}
