import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { api, type DeviceInfo, type DeviceTarget } from '../api';
import { closeOverlay, openOverlay } from '../components/overlay';

/** What the picker is for: it only changes the window's title. */
export type DevicePurpose = 'destination' | 'import' | 'copy';

const TITLES: Record<DevicePurpose, string> = {
  destination: 'devicePicker.title',
  import: 'devicePicker.importWindowTitle',
  copy: 'devicePicker.copyWindowTitle',
};

/** How often the list of phones is asked again, so plugging one in shows up without reopening. */
const POLL_MS = 4000;

/** "Pixel 8 / Music/Soundify", or "(raíz)" at the root: what a chosen destination reads as. */
export function deviceLabel(target: DeviceTarget): string {
  return t('download.deviceChosen', target.deviceName, target.path || t('devicePicker.root'));
}

/**
 * Choose a phone connected over USB (MTP) and a folder on it. Only folders are
 * listed; `path` comes back "/"-joined, "" being the phone's root.
 */
export function openDevicePicker(purpose: DevicePurpose, onPicked: (target: DeviceTarget) => void): void {
  let devices: DeviceInfo[] = [];
  let path: string[] = [];
  // Each folder listing gets a number: an answer for a folder already left is dropped.
  let request = 0;
  // While a folder is being listed the phone is busy: the poll waits its turn.
  let listing = false;

  const status = h('p', { class: 'help', text: t('devicePicker.loading') });
  const select = h('select', { class: 'input' });
  const pathLabel = h('div', { class: 'row-title ellipsis' });
  const list = h('div', { class: 'device-list' });
  const up = h('button', { class: 'secondary-button', text: t('devicePicker.up'), onclick: () => {
    path.pop();
    void loadFolder();
  } });
  const choose = h('button', { class: 'primary-button', text: t('devicePicker.selectHere'), onclick: () => {
    const device = current();
    if (!device) return;
    closeOverlay();
    onPicked({ deviceId: device.id, deviceName: device.name, path: path.join('/') });
  } });
  const browser = h('div', { class: 'stack-12' },
    h('div', { class: 'section-label', text: t('devicePicker.device') }), select,
    h('div', { class: 'row center gap-8' }, up, pathLabel), list);

  select.addEventListener('change', () => {
    path = [];
    void loadFolder();
  });

  const current = (): DeviceInfo | undefined => devices.find(d => d.id === select.value);

  /** Asks for the phones again; when the chosen one is gone, the next one starts at its root. */
  const refreshDevices = async () => {
    if (listing) return;
    const view = await api.listDevices().catch(() => ({ available: false, devices: [] }));
    const before = select.value;
    devices = view.devices;
    const empty = devices.length === 0;
    status.textContent = t('devicePicker.noDevices');
    status.hidden = !empty;
    browser.hidden = empty;
    choose.disabled = empty;
    select.replaceChildren(...devices.map(d => h('option', { text: d.name, attrs: { value: d.id } })));
    if (empty) return;
    if (devices.some(d => d.id === before)) {
      select.value = before;
      return;
    }
    path = [];
    void loadFolder();
  };

  const loadFolder = async () => {
    const device = current();
    if (!device) return;
    const asked = ++request;
    listing = true;
    up.disabled = path.length === 0;
    pathLabel.textContent = deviceLabel({ deviceId: device.id, deviceName: device.name, path: path.join('/') });
    list.replaceChildren(h('p', { class: 'help', text: t('devicePicker.loading') }));
    const entries = await api.listDeviceFolder(device.id, path.join('/')).catch(() => []);
    if (asked !== request) return;
    listing = false;
    const folders = entries.filter(e => e.isFolder).map(e => e.name)
      .sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' }));
    list.replaceChildren(...(folders.length
      ? folders.map(name => h('button', { class: 'device-folder', onclick: () => {
        path.push(name);
        void loadFolder();
      } }, icon('folder', 16), h('span', { text: name })))
      : [h('p', { class: 'help', text: t('devicePicker.emptyFolder') })]));
  };

  browser.hidden = true;
  choose.disabled = true;
  const poll = window.setInterval(() => void refreshDevices(), POLL_MS);
  openOverlay(t(TITLES[purpose]), [
    purpose === 'destination' ? h('p', { class: 'subtitle', text: t('devicePicker.description') }) : null,
    status,
    browser,
    deviceLimits(),
    h('div', { class: 'row end' }, choose),
  ].filter((node): node is HTMLElement => node !== null), { onClose: () => window.clearInterval(poll) });
  void refreshDevices();
}

/**
 * «¿Problemas con el dispositivo?»: folded inside the picker. The Java app
 * opened a window of its own; here a second overlay would close the picker.
 */
function deviceLimits(): HTMLElement {
  const steps = [1, 2, 3, 4].map(n => h('div', { class: 'help-step' },
    h('span', { class: 'help-step-number', text: String(n) }),
    h('p', { text: t(`help.deviceLimits.step${n}`) })));
  return h('details', { class: 'device-limits' },
    h('summary', { class: 'help', text: t('help.deviceLimits.link') }),
    h('div', { class: 'stack' },
      h('div', { class: 'card-title', text: t('help.deviceLimits.title') }),
      h('p', { class: 'subtitle', text: t('help.deviceLimits.intro') }),
      ...steps,
      h('p', { class: 'help-note', text: t('help.deviceLimits.note') })));
}
