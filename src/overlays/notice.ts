// An explanatory window with «No volver a mostrar este aviso»: it asks (or just explains) before something happens, and
// once the user ticks the box it never shows again and the action runs straight away. Ajustes → General → Avisos y guías
// brings every one back (`preferences.ts`).

import { h } from '../dom';
import { t } from '../i18n';
import { noticeHidden, hideNotice } from '../preferences';
import { confirmDialog } from './confirm';

export interface NoticeOptions {
  /** Stable id of this notice (what is stored when it is hidden): never reuse one for another window. */
  id: string;
  title: string;
  header: string;
  body: string;
  /** The button that goes ahead. */
  confirm: string;
  run: () => void;
  /** Danger-coloured instead of the accent. */
  danger?: boolean;
}

/** Shows the window, or goes straight to `run` when the user hid it. */
export function showNotice(options: NoticeOptions): void {
  if (noticeHidden(options.id)) {
    options.run();
    return;
  }
  const box = h('input', { attrs: { type: 'checkbox', id: `notice-${options.id}` } });
  const remember = h('label', { class: 'notice-remember row center gap-8' }, box, h('span', { text: t('notice.dontShowAgain') }));
  confirmDialog(options.title, options.header, options.body, [
    { label: options.confirm, kind: options.danger ? 'danger' : 'primary', run: () => {
      if (box.checked) hideNotice(options.id);
      options.run();
    } },
  ], remember);
}
