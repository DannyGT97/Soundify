// Pieces shared by "Descarga por URL" and "Crear Playlist": the format list,
// the destination card and the summary panel on the right.

import { h, stepHeading } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { api, type DeviceTarget } from '../api';
import { library } from '../library';
import { ui } from '../state';
import { deviceLabel, openDevicePicker } from '../overlays/devicePicker';

/** One list for every format; options are grouped by audio and video type. */
export function formatSelect(): HTMLSelectElement {
  const { audio, video } = library.formats;
  return h('select', { class: 'input' },
    ...audio.map(f => h('option', { text: f, attrs: { value: f } })),
    ...video.map(f => h('option', { text: f, attrs: { value: f } })));
}

export interface Destination {
  card: HTMLElement;
  /** The folder, or how the chosen phone folder reads ("Pixel 8 / Music"); empty = nothing chosen. */
  field: HTMLInputElement;
  /** The phone folder when that is what was chosen; null for a folder of this computer. */
  device: () => DeviceTarget | null;
}

/** "02 Guardar en": the path on its own row and the two buttons below, half and half. */
export function destinationCard(step: string, placeholderKey: string, changed: (text: string) => void): Destination {
  const field = h('input', { class: 'input', attrs: { readonly: '', placeholder: t(placeholderKey) } });
  let device: DeviceTarget | null = null;
  const set = (text: string, target: DeviceTarget | null) => {
    field.value = text;
    device = target;
    changed(text);
  };
  const folderButton = h('button', { class: 'secondary-button grow', onclick: async () => {
    const chosen = await api.pickFolder(t('download.folder.choose'));
    if (chosen) set(chosen, null);
  } },
    icon('folder', 16), h('span', { text: t('download.chooseFolder') }));
  const deviceButton = h('button', { class: 'secondary-button grow', title: t('download.chooseDevice.tooltip'),
    onclick: () => openDevicePicker('destination', target => set(deviceLabel(target), target)) },
    icon('device', 16), h('span', { text: t('download.chooseDevice') }));
  return {
    card: h('section', { class: 'card' }, stepHeading(step, t('download.saveIn')), field,
      h('div', { class: 'row gap-8' }, folderButton, deviceButton)),
    field,
    device: () => device,
  };
}

export interface Summary {
  panel: HTMLElement;
  showButton: HTMLButtonElement;
  /** Opens the panel; used only when the field turns into a usable URL, so hiding it by hand is respected. */
  open: () => void;
}

/**
 * The summary on the right, hidden with its » and brought back with the « of
 * the title row. ONE flag for both screens, so it is hidden or shown in both.
 */
export function summaryPanel(titleKey: string, art: HTMLElement, rows: Node[], noticeKey: string): Summary {
  const hide = h('button', { class: 'icon-button', title: t('summary.hide') }, icon('summary-hide'));
  const showButton = h('button', { class: 'icon-button', title: t('summary.show') }, icon('summary-show'));
  const panel = h('aside', { class: 'panel summary' },
    h('div', { class: 'row center gap-8' }, h('div', { class: 'summary-title', text: t(titleKey) }), h('div', { class: 'grow' }), hide),
    art, ...rows, h('div', { class: 'grow' }), h('p', { class: 'help', text: t(noticeKey) }));
  const apply = () => {
    panel.hidden = ui.summaryHidden;
    showButton.hidden = !ui.summaryHidden;
  };
  hide.addEventListener('click', () => { ui.summaryHidden = true; apply(); });
  showButton.addEventListener('click', () => { ui.summaryHidden = false; apply(); });
  apply();
  return { panel, showButton, open: () => { ui.summaryHidden = false; apply(); } };
}

/** A label and its value in the summary, with the separator above. */
export function summaryRow(labelKey: string, value: HTMLElement, first = false): Node[] {
  return [first ? null : h('hr'), h('div', { class: 'section-label', text: t(labelKey) }), value]
      .filter((n): n is HTMLElement => n !== null);
}

export function summaryArt(): HTMLElement {
  return h('div', { class: 'summary-art' }, h('span', { class: 'summary-note' }, icon('audio', 40)));
}
