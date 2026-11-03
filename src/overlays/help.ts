import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { closeOverlay, openOverlay } from '../components/overlay';
import { importFromDevice, importFromFolder } from './transfer';

/**
 * The help of "Importar". Its bottom button IS the action it explains: the
 * explanation is not a step in the way, it is the way in.
 */
export function openImportHelp(): void {
  const steps = [1, 2, 3, 4].map(n => h('div', { class: 'help-step' },
    h('span', { class: 'help-step-number', text: String(n) }),
    h('p', { text: t(`help.import.step${n}`) })));
  const explore = h('button', { class: 'primary-button', onclick: () => {
    closeOverlay();
    void importFromFolder();
  } },
    icon('folder', 16), h('span', { text: t('help.import.action') }));
  openOverlay(t('help.import.title'), [
    h('p', { class: 'subtitle', text: t('help.import.intro') }),
    h('div', { class: 'stack' }, ...steps),
    h('p', { class: 'help-note', text: t('help.import.note') }),
    h('div', { class: 'row center-x' }, explore),
    h('div', { class: 'stack center-text' },
      h('p', { class: 'help', text: t('help.import.deviceNote') }),
      h('div', { class: 'row center-x' },
        h('button', { class: 'secondary-button', text: t('help.import.deviceAction'), onclick: () => {
          closeOverlay();
          importFromDevice();
        } }))),
  ]);
}
