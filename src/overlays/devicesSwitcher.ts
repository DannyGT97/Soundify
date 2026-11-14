// The speaker icon of the bar and the device chip of Reproduciendo: only the list of devices, to switch between them
// (this one, or the one to control). Everything about managing them (pairing, PIN, firewall, address) is in Ajustes:
// «¿No ves tu dispositivo?» goes there.

import { closeOverlay, closeOverlayThen, openOverlay } from '../components/overlay';
import { connect, controlDevice, loadConnect, stopControlling } from '../connect';
import { h } from '../dom';
import { t } from '../i18n';
import { icon, type IconName } from '../icons';
import { onAndroid } from '../platform';
import { navigate } from '../router';
import { subscribe, ui } from '../state';
import { openDevicesOverlay } from './devices';

const KIND_ICON: Record<string, IconName> = { desktop: 'monitor', phone: 'device', tablet: 'device' };

export function openDevicesSwitcher(): void {
  const body = h('div', { class: 'stack-12 cx' });
  // Redrawn only when what it shows changed: a row that is being pressed must not be swapped under the finger.
  let shown = '';
  const refresh = () => {
    const nodes = content();
    const signature = nodes.map(node => (node as HTMLElement).outerHTML ?? '').join('');
    if (signature === shown) return;
    shown = signature;
    body.replaceChildren(...nodes);
  };
  const unsubscribe = subscribe('connect', refresh);
  void loadConnect();
  refresh();
  openOverlay(t('connect.title'), [body], { onClose: unsubscribe });
}

function row(options: { name: string; kind: string; state: string; current: boolean; enabled: boolean; id: string; pick: () => void }): HTMLElement {
  const element = h('div', {
    class: `card cx-device cx-pick${options.current ? ' connected' : ''}${options.enabled ? '' : ' offline'}`,
    attrs: { 'data-device': options.id, role: 'button', tabindex: options.enabled ? '0' : '-1' },
  },
    icon(KIND_ICON[options.kind] ?? 'device', 20),
    h('div', { class: 'grow min0' },
      h('div', { class: 'row-title ellipsis', text: options.name }),
      h('div', { class: 'subtitle ellipsis', text: options.state })),
    options.current ? icon('ok', 18) : null);
  if (options.enabled) {
    element.addEventListener('click', options.pick);
    element.addEventListener('keydown', event => {
      if (event.key !== 'Enter') return;
      event.preventDefault();
      options.pick();
    });
  }
  return element;
}

function content(): Node[] {
  const status = connect.status;
  const nodes: Node[] = [];
  if (!status?.running) {
    nodes.push(h('p', { class: 'help', text: t('connect.switch.off') }));
  } else {
    nodes.push(row({
      id: 'this', name: status.name, kind: onAndroid ? 'phone' : 'desktop', current: !ui.remote, enabled: true,
      state: t('connect.switch.thisDevice'),
      pick: () => { void (ui.remote ? stopControlling() : Promise.resolve()).then(() => closeOverlay()); },
    }));
    for (const device of connect.devices.filter(other => other.paired || other.online)) {
      const ready = device.online && device.ready;
      const state = device.playing && device.nowPlaying ? t('connect.playing', device.nowPlaying.title)
        : device.connected ? t('connect.state.controlled')
        : !device.online ? t('connect.state.offline')
        : ready ? t('connect.state.ready')
        : device.needsPin ? t('connect.state.new')
        : t('connect.state.connecting');
      nodes.push(row({
        id: device.id, name: device.name, kind: device.kind, state, current: device.connected, enabled: ready,
        pick: () => { void (device.connected ? Promise.resolve(true) : controlDevice(device)).then(ok => ok && closeOverlay()); },
      }));
    }
  }
  nodes.push(h('div', { class: 'cx-help-line' },
    h('span', { class: 'help', text: t('connect.switch.help') }),
    h('button', { class: 'text-button', text: t('connect.switch.manage'), onclick: () => {
      closeOverlayThen(() => {
        navigate('settings');
        openDevicesOverlay();
      });
    } })));
  return nodes;
}
