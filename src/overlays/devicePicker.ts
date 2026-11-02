import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { closeOverlay, openOverlay } from '../components/overlay';

// PLACEHOLDER: one fake phone and its folders until MTP is wired.
const FOLDERS = ['Music', 'Download', 'DCIM', 'Podcasts'];

/** Choose a phone connected over USB (MTP) and a folder on it. */
export function openDevicePicker(onPicked?: (path: string) => void): void {
  const path: string[] = [];
  const pathLabel = h('div', { class: 'row-title' });
  const list = h('div', { class: 'device-list' });

  const refresh = () => {
    pathLabel.textContent = `Pixel 8 \\ ${path.join(' \\ ')}`;
    const folders = path.length < 2 ? FOLDERS : [];
    list.replaceChildren(...folders.map(name => h('button', { class: 'device-folder', onclick: () => {
      path.push(name);
      refresh();
    } }, icon('folder', 16), h('span', { text: name }))));
  };
  refresh();

  openOverlay(t('devicePicker.title'), [
    h('p', { class: 'subtitle', text: t('devicePicker.description') }),
    h('div', { class: 'section-label', text: t('devicePicker.device') }),
    h('select', { class: 'input' }, h('option', { text: 'Pixel 8' })),
    h('div', { class: 'row center gap-8' },
      h('button', { class: 'secondary-button', text: t('devicePicker.up'), onclick: () => {
        path.pop();
        refresh();
      } }),
      pathLabel),
    list,
    h('div', { class: 'row end' },
      h('button', { class: 'primary-button', text: t('devicePicker.selectHere'), onclick: () => {
        closeOverlay();
        onPicked?.(`Pixel 8\\${path.join('\\')}`);
      } })),
  ]);
}
