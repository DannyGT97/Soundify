// A guide: a short window of one or more steps that explains something THE FIRST TIME it matters (the first start, the
// first time Dispositivos opens, the first pass to another device...). Seen once, it does not come back by itself; Ajustes →
// General → «Restablecer avisos y guías» brings every one back. It shares the list of hidden notices (`preferences.ts`), so
// «Ocultas ahora» counts them too.
//
// `run` is what the guide stands in front of (opening the window, passing the music): it goes ahead when the guide ends with
// its last button, and straight away when the guide was seen already. Closing it any other way (✕, Escape) marks it as seen
// and does not go ahead. `after` is for what comes next whatever happened (the next guide of the first start).

import { closeOverlay, openOverlay } from '../components/overlay';
import { h } from '../dom';
import { t } from '../i18n';
import { icon, type IconName } from '../icons';
import { hideNotice, noticeHidden } from '../preferences';

export interface GuideStep {
  icon: IconName;
  title: string;
  body: string;
}

export interface GuideOptions {
  /** Stable id (what is stored when it was seen): never reuse one for another guide. */
  id: string;
  title: string;
  steps: GuideStep[];
  /** The last button («Empezar», «Entendido»). */
  done: string;
  run?: () => void;
  after?: () => void;
}

/** Shows the guide unless it was seen; then (or if it was) `run` and `after` happen as said above. */
export function showGuide(options: GuideOptions): void {
  if (noticeHidden(options.id) || options.steps.length === 0) {
    options.run?.();
    options.after?.();
    return;
  }
  let index = 0;
  let finished = false;
  const body = h('div', { class: 'guide' });
  const draw = () => {
    const step = options.steps[index];
    if (!step) return;
    const last = index === options.steps.length - 1;
    const dots = options.steps.length > 1
      ? h('div', { class: 'guide-dots', attrs: { 'aria-hidden': 'true' } },
        ...options.steps.map((_, n) => h('span', { class: `guide-dot${n === index ? ' on' : ''}` })))
      : null;
    const back = index > 0 ? h('button', { class: 'secondary-button', text: t('guide.back'), onclick: () => { index -= 1; draw(); } }) : null;
    const forward = h('button', {
      class: 'primary-button',
      text: last ? options.done : t('guide.next'),
      onclick: () => {
        if (!last) {
          index += 1;
          draw();
          return;
        }
        finished = true;
        closeOverlay();
      },
    });
    const actions: Node[] = back ? [back, forward] : [forward];
    const parts: Node[] = [
      h('div', { class: 'guide-art' }, icon(step.icon, 34)),
      h('h3', { class: 'guide-title', text: step.title }),
      h('p', { class: 'guide-body pre-line', text: step.body }),
    ];
    if (dots) parts.push(dots);
    parts.push(h('div', { class: 'guide-actions' }, ...actions));
    body.replaceChildren(...parts);
    forward.focus();
  };
  draw();
  openOverlay(options.title, [body], {
    onClose: () => {
      hideNotice(options.id);
      if (finished) options.run?.();
      options.after?.();
    },
  });
}
