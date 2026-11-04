import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { closeButton, closeOverlay, openPanel } from '../components/overlay';
import { importFromDevice, importFromFolder } from './transfer';

/**
 * Importar: the two places a playlist can come from, and «¿Qué carpeta elijo?»
 * folded. The window keeps its size when that opens: the two cards give up
 * the room.
 */
export function openImportHelp(): void {
  const body = h('div', { class: 'stack-12' },
    h('div', { class: 'tree', attrs: { 'aria-hidden': 'true' } },
      h('div', {}, icon('folder', 14), h('span', { class: 'ellipsis', text: t('import.tree.music') }), h('span', { class: 'here', text: t('import.tree.all') })),
      h('div', { attrs: { style: 'margin-left:18px' } }, icon('folder', 14), h('span', { class: 'ellipsis', text: t('import.tree.playlist') }), h('span', { class: 'here', text: t('import.tree.this') })),
      h('div', { attrs: { style: 'margin-left:36px' } }, icon('file-json', 14), h('span', { class: 'key', text: 'soundify-playlist.json' })),
      h('div', { attrs: { style: 'margin-left:36px' } }, icon('file-music', 14), h('span', { text: '001 - Neon Rain.mp3' }))),
    h('ul', { class: 'points' },
      h('li', { text: t('import.point.pick') }),
      h('li', { text: t('import.point.stays') }),
      h('li', { text: t('import.point.copy') })));
  body.hidden = true;
  const chevron = h('span', { class: 'chevron' }, icon('chevron', 16));
  const fold = h('button', { class: 'fold', attrs: { 'aria-expanded': 'false' }, onclick: () => {
    body.hidden = !body.hidden;
    chevron.classList.toggle('open', !body.hidden);
    fold.setAttribute('aria-expanded', String(!body.hidden));
  } }, chevron, h('span', { text: t('import.which') }));

  const way = (iconName: 'general' | 'device', title: string, text: string, button: HTMLElement) =>
    h('div', { class: 'way' },
      h('span', { class: 'way-ic' }, icon(iconName, 22)),
      h('b', { text: title }),
      h('p', { class: 'muted', text }),
      button);

  openPanel(h('div', { class: 'mod import' },
    h('div', { class: 'mod-head' },
      h('h2', { class: 'grow', text: t('help.import.title') }),
      closeButton()),
    h('div', { class: 'mod-body' },
      h('p', { class: 'muted', text: t('import.intro') }),
      h('div', { class: 'ways' },
        way('general', t('import.computer'), t('import.computer.text'),
          h('button', { class: 'primary-button', text: t('help.import.action'), onclick: () => {
            closeOverlay();
            void importFromFolder();
          } })),
        way('device', t('import.device'), t('import.device.text'),
          h('button', { class: 'secondary-button', text: t('help.import.deviceAction'), onclick: () => {
            closeOverlay();
            importFromDevice();
          } }))),
      fold,
      body)));
}
