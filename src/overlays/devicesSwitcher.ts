// The speaker icon of the bar and the device chip of Reproduciendo: only the list of devices, to switch between them.
// Two different things, in two sections with their own colour and icon (`design/connect-cast-prototype.html`):
//   «Escuchar en» (green, cast): passes what plays HERE to another device, which reads the files from this one (`cast.ts`).
//   «Controlar otro dispositivo» (blue, remote): drives what ANOTHER device plays, from its own music (`connect.ts`).
// Above them, while one of the two is going on, a box says so with the ways out. Everything about managing devices
// (pairing, PIN, firewall, address) is in Ajustes: «¿No ves tu dispositivo?» goes there.

import type { ConnectDevice } from '../api';
import { closeOverlay, closeOverlayThen, openOverlay } from '../components/overlay';
import { bringHere, canPass, passTo, stopPass } from '../cast';
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
  const body = h('div', { class: 'stack-12 cx cx-switcher' });
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
  // The player ticks several times a second: only whether there is something to pass changes what this shows.
  let couldPass = canPass();
  const unsubscribePlayer = subscribe('player', () => {
    if (canPass() === couldPass) return;
    couldPass = canPass();
    refresh();
  });
  void loadConnect();
  refresh();
  openOverlay(t('connect.title'), [body], { onClose: () => { unsubscribe(); unsubscribePlayer(); } });
}

interface Row {
  /** `data-pass` (Escuchar en) or `data-device` (Controlar), with the device's id. */
  attr: 'data-pass' | 'data-device';
  id: string;
  name: string;
  kind: string;
  state: string;
  current: boolean;
  enabled: boolean;
  /** What the end of the row says it does («Pasar aquí», «Controlar»), when it is not the current one. */
  go?: string;
  pick: () => void;
}

function row(options: Row): HTMLElement {
  const element = h('div', {
    class: `card cx-device cx-pick${options.current ? ' connected' : ''}${options.enabled ? '' : ' offline'}`,
    attrs: { [options.attr]: options.id, role: 'button', tabindex: options.enabled ? '0' : '-1' },
  },
    h('span', { class: 'cx-device-icon' }, icon(KIND_ICON[options.kind] ?? 'device', 20)),
    h('div', { class: 'grow min0' },
      h('div', { class: 'row-title ellipsis', text: options.name }),
      h('div', { class: 'subtitle ellipsis', text: options.state })),
    options.current ? h('span', { class: 'cx-end' }, icon('ok', 18))
      : options.go && options.enabled ? h('span', { class: 'cx-go', text: options.go }) : null);
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

function section(kind: 'cast' | 'control', title: string, help: string): Node[] {
  return [
    h('div', { class: `cx-section cx-section-${kind}` }, icon(kind === 'cast' ? 'cast' : 'control', 16), h('span', { text: title })),
    h('p', { class: 'help cx-hint', text: help }),
  ];
}

/** Why a device of the list cannot be used now (or that it can). */
function stateOf(device: ConnectDevice): string {
  if (device.version === 'outdated') return t('connect.state.outdated');
  if (device.version === 'newer') return t('connect.state.newer');
  if (!device.online) return t('connect.state.offline');
  if (device.needsPin) return t('connect.state.new');
  if (!device.ready) return t('connect.state.connecting');
  return '';
}

function content(): Node[] {
  const status = connect.status;
  const nodes: Node[] = [];
  if (!status?.running) {
    nodes.push(h('p', { class: 'help', text: t('connect.switch.off') }));
    nodes.push(footer());
    return nodes;
  }
  const devices = connect.devices.filter(other => other.paired || other.online);
  const ready = (device: ConnectDevice) => device.online && device.ready && !device.version;

  // What is going on now, with the ways out.
  if (ui.castTo) {
    nodes.push(h('div', { class: 'card cx-banner cx-banner-cast', attrs: { 'data-casting': ui.castTo.deviceId } },
      h('div', { class: 'cx-banner-head' }, icon('cast', 20),
        h('div', { class: 'min0' },
          h('div', { class: 'row-title', text: t('connect.cast.playingOn', ui.castTo.name) }),
          h('div', { class: 'subtitle', text: t('connect.cast.playingOnHelp') }))),
      h('div', { class: 'cx-banner-actions' },
        h('button', { class: 'primary-button', text: t('connect.cast.bring'), onclick: () => { void bringHere().then(() => closeOverlay()); } }),
        h('button', { class: 'secondary-button', text: t('connect.cast.stop'), onclick: () => { void stopPass().then(() => closeOverlay()); } }))));
  } else if (ui.remote) {
    nodes.push(h('div', { class: 'card cx-banner cx-banner-control', attrs: { 'data-controlling': ui.remote.deviceId } },
      h('div', { class: 'cx-banner-head' }, icon('control', 20),
        h('div', { class: 'min0' },
          h('div', { class: 'row-title', text: t('connect.control.banner', ui.remote.name) }),
          h('div', { class: 'subtitle', text: t('connect.control.bannerHelp') }))),
      h('div', { class: 'cx-banner-actions' },
        h('button', { class: 'secondary-button', text: t('connect.control.release'), onclick: () => { void stopControlling().then(() => closeOverlay()); } }))));
  }

  // ESCUCHAR EN: where what plays here sounds.
  const passing = canPass();
  nodes.push(...section('cast', t('connect.cast.section'), t('connect.cast.help')));
  if (!passing && !ui.castTo) {
    nodes.push(h('p', { class: 'help cx-empty', text: ui.remote ? t('connect.cast.whileControlling', ui.remote.name) : t('connect.cast.needsMusic') }));
  } else {
    nodes.push(row({
      attr: 'data-pass', id: 'this', name: status.name, kind: onAndroid ? 'phone' : 'desktop',
      state: ui.castTo ? t('connect.cast.bringHere') : t('connect.cast.here'),
      current: !ui.castTo, enabled: true,
      pick: () => { void (ui.castTo ? bringHere() : Promise.resolve()).then(() => closeOverlay()); },
    }));
    for (const device of devices) {
      const usable = ready(device);
      const mine = ui.castTo?.deviceId === device.id;
      nodes.push(row({
        attr: 'data-pass', id: device.id, name: device.name, kind: device.kind,
        state: mine ? t('connect.cast.playingYours') : usable ? t('connect.cast.ready') : stateOf(device),
        current: mine, enabled: usable && !mine, go: t('connect.cast.passHere'),
        pick: () => { void passTo(device).then(ok => ok && closeOverlay()); },
      }));
    }
  }

  // CONTROLAR: what another device plays, from its own music. A device with this one's music is not listed here.
  const controllable = devices.filter(device => ready(device) && device.id !== ui.castTo?.deviceId);
  nodes.push(...section('control', t('connect.control.section'), t('connect.control.help')));
  if (!controllable.length) nodes.push(h('p', { class: 'help cx-empty', text: t('connect.control.none') }));
  for (const device of [...controllable].sort((a, b) => Number(b.playing) - Number(a.playing))) {
    const current = device.connected && !ui.castTo;
    nodes.push(row({
      attr: 'data-device', id: device.id, name: device.name, kind: device.kind,
      state: device.playing && device.nowPlaying ? t('connect.playing', device.nowPlaying.title) : current ? t('connect.state.controlled') : t('connect.control.idle'),
      current, enabled: true, go: t('connect.control.go'),
      pick: () => { void (current ? Promise.resolve(true) : controlDevice(device)).then(ok => ok && closeOverlay()); },
    }));
  }

  nodes.push(footer());
  return nodes;
}

function footer(): HTMLElement {
  return h('div', { class: 'cx-help-line' },
    h('span', { class: 'help', text: t('connect.switch.help') }),
    h('button', { class: 'text-button', text: t('connect.switch.manage'), onclick: () => {
      closeOverlayThen(() => {
        navigate('settings');
        openDevicesOverlay();
      });
    } }));
}
