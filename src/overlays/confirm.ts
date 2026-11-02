import { h } from '../dom';
import { t } from '../i18n';
import { closeOverlay, openOverlay } from '../components/overlay';

export interface ConfirmAction {
  label: string;
  kind?: 'primary' | 'danger' | 'secondary';
  run?: () => void;
}

/**
 * A yes/no (or several-button) question; the last button is always Cancel.
 * `extra` goes between the text and the buttons (a checkbox the action reads).
 */
export function confirmDialog(title: string, header: string, body: string, actions: ConfirmAction[], extra?: Node): void {
  const buttons = actions.map(action => h('button', {
    class: action.kind === 'danger' ? 'danger-button' : action.kind === 'primary' ? 'primary-button' : 'secondary-button',
    text: action.label,
    onclick: () => {
      closeOverlay();
      action.run?.();
    },
  }));
  openOverlay(title, [
    h('div', { class: 'card-title', text: header }),
    body ? h('p', { class: 'help pre-line', text: body }) : null,
    extra ?? null,
    h('div', { class: 'row wrap end gap-8' }, ...buttons,
      h('button', { class: 'secondary-button', text: t('common.cancel'), onclick: () => closeOverlay() })),
  ].filter((node): node is Node => node !== null));
}
