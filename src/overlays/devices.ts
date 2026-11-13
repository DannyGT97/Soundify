// «Dispositivos»: Soundify Connect. This device's own PIN (so another can control it), the
// devices on the network to pair with and control, handing the playback over, and who may
// control this one. It redraws on `connect` and keeps what is typed in the PIN boxes.

import { api, type ConnectDevice } from '../api';
import { notify } from '../components/toasts';
import { closeOverlay, openOverlay } from '../components/overlay';
import { bringHere, connect, controlDevice, loadConnect, stopControlling, transferTo } from '../connect';
import { h } from '../dom';
import { t } from '../i18n';
import { icon, type IconName } from '../icons';
import { onAndroid } from '../platform';
import { subscribe, ui } from '../state';
import { errorText } from '../transfer';

/** What was typed in each device's PIN box, so a redraw does not empty it. */
const typed = new Map<string, string>();
let ownPin: string | null = null;

export function openDevicesOverlay(): void {
  const body = h('div', { class: 'stack-12 cx' });
  const refresh = () => body.replaceChildren(...content());
  const unsubscribe = subscribe('connect', refresh);
  const reload = () => void loadConnect().then(() => refreshPin().then(refresh));
  reload();
  // Someone may pair with this device meanwhile (the PIN is used up, the list of controllers grows): looked at again now and then.
  const poll = window.setInterval(reload, 3000);
  refresh();
  openOverlay(t('connect.title'), [body], { onClose: () => { unsubscribe(); window.clearInterval(poll); } });
}

async function refreshPin(renew = false): Promise<void> {
  ownPin = connect.status?.running ? await (renew ? api.connectRenewPin() : api.connectPin()).catch(() => null) : null;
}

const KIND_ICON: Record<ConnectDevice['kind'], IconName> = { desktop: 'monitor', phone: 'device', tablet: 'device' };

function content(): Node[] {
  const status = connect.status;
  const nodes: Node[] = [h('p', { class: 'help', text: t('connect.intro') })];
  nodes.push(h('div', { class: 'cx-switch row center gap-10' },
    h('div', { class: 'grow' },
      h('div', { class: 'section-title', text: t('connect.enable') }),
      h('p', { class: 'subtitle', text: t(status?.enabled ? 'connect.enable.on' : 'connect.enable.off', status?.name ?? '') })),
    connectSwitch()));
  if (!status?.running) return nodes;

  // This device: who it is, and the way to bring the playback back from another one.
  nodes.push(h('div', { class: 'section-label', text: t('connect.thisDevice') }));
  nodes.push(h('div', { class: 'card cx-device' },
    icon(onAndroid ? 'device' : 'monitor', 20),
    h('div', { class: 'grow min0' },
      h('div', { class: 'row-title ellipsis', text: status.name }),
      h('div', { class: 'subtitle', text: ui.remote ? t('connect.controlling', ui.remote.name) : t('connect.soundsHere') })),
    ui.remote ? h('button', { class: 'primary-button', text: t('connect.bringHere'), onclick: () => void bringHere() }) : null));

  nodes.push(pinCard());

  nodes.push(h('div', { class: 'section-label', text: t('connect.others') }));
  const list = connect.devices;
  if (!list.length) nodes.push(h('p', { class: 'help', text: t('connect.none') }));
  for (const device of list) nodes.push(deviceRow(device));
  nodes.push(addByAddress());

  if (status.controllers.length) {
    nodes.push(h('div', { class: 'section-label', text: t('connect.controllers') }));
    for (const controller of status.controllers) {
      nodes.push(h('div', { class: 'card cx-device' },
        h('div', { class: 'grow min0' }, h('div', { class: 'row-title ellipsis', text: controller.name })),
        h('button', { class: 'text-button danger-text', text: t('connect.revoke'), onclick: () => void run(api.connectForgetController(controller.id)) })));
    }
  }
  return nodes;
}

/** The PIN another device types to pair with this one. */
function pinCard(): HTMLElement {
  const text = ownPin ? `${ownPin.slice(0, 3)} ${ownPin.slice(3)}` : '—';
  return h('div', { class: 'card cx-pin' },
    h('div', { class: 'grow' },
      h('div', { class: 'section-title', text: t('connect.pin.title') }),
      h('p', { class: 'subtitle', text: t('connect.pin.desc') })),
    h('div', { class: 'cx-pin-code', text }),
    h('button', { class: 'text-button', text: t('connect.pin.renew'), onclick: () => void refreshPin(true).then(() => void loadConnect()) }));
}

function deviceRow(device: ConnectDevice): HTMLElement {
  const state = device.connected ? t('connect.state.controlled') : device.online ? t(device.paired ? 'connect.state.paired' : 'connect.state.new') : t('connect.state.offline');
  const actions = h('div', { class: 'row wrap gap-8 cx-actions' });
  if (device.connected) {
    actions.append(h('button', { class: 'secondary-button', text: t('connect.stop'), onclick: () => void stopControlling() }));
  } else if (device.online && device.paired) {
    actions.append(
      h('button', { class: 'primary-button', text: t('connect.control'), onclick: () => void controlDevice(device).then(ok => ok && closeOverlay()) }),
      h('button', { class: 'secondary-button', text: t('connect.handOver'), onclick: () => void transferTo(device).then(() => closeOverlay()) }));
  } else if (device.online) {
    const pin = h('input', { class: 'input cx-pin-input', attrs: { inputmode: 'numeric', maxlength: '8', placeholder: t('connect.pair.placeholder'), 'aria-label': t('connect.pair.placeholder') } });
    pin.value = typed.get(device.id) ?? '';
    const pair = () => {
      const code = pin.value.replace(/\D/g, '');
      if (code.length !== 6) return notify({ tone: 'warning', text: t('connect.pair.six') });
      void run(api.connectPair(device.id, code).then(() => { typed.delete(device.id); notify({ tone: 'success', text: t('connect.pair.done', device.name) }); }));
    };
    pin.addEventListener('input', () => typed.set(device.id, pin.value));
    pin.addEventListener('keydown', event => event.key === 'Enter' && pair());
    actions.append(pin, h('button', { class: 'primary-button', text: t('connect.pair'), onclick: pair }));
  }
  if (device.paired) actions.append(h('button', { class: 'text-button danger-text', text: t('connect.forget'), onclick: () => void run(api.connectForget(device.id)) }));
  return h('div', { class: `card cx-device${device.connected ? ' connected' : ''}${device.online ? '' : ' offline'}`, attrs: { 'data-device': device.id } },
    icon(KIND_ICON[device.kind] ?? 'device', 20),
    h('div', { class: 'grow min0' },
      h('div', { class: 'row-title ellipsis', text: device.name }),
      h('div', { class: 'subtitle', text: device.address ? `${state} · ${device.address}` : state }),
      actions));
}

/** When the devices do not find each other (some routers block mDNS): the address the other one shows. */
function addByAddress(): HTMLElement {
  const input = h('input', { class: 'input', attrs: { placeholder: t('connect.add.placeholder'), 'aria-label': t('connect.add.placeholder') } });
  const add = () => {
    const address = input.value.trim();
    if (address) void run(api.connectAdd(address).then(() => { input.value = ''; }));
  };
  input.addEventListener('keydown', event => event.key === 'Enter' && add());
  return h('div', { class: 'cx-add' },
    h('p', { class: 'help', text: t('connect.add.desc') }),
    h('div', { class: 'row gap-8' }, input, h('button', { class: 'secondary-button', text: t('connect.add'), onclick: add })));
}

/** Waits for what the back was asked and tells the user if it failed; the list redraws on its own. */
async function run(call: Promise<unknown>): Promise<void> {
  try {
    await call;
  } catch (error) {
    notify({ tone: 'error', text: errorText(error) });
  }
  void loadConnect();
}

/** The switch that turns Soundify Connect on or off (the same in Ajustes and in this window). */
export function connectSwitch(): HTMLInputElement {
  const box = h('input', { class: 'switch', attrs: { type: 'checkbox' } });
  box.checked = connect.status?.enabled ?? false;
  box.addEventListener('change', () => {
    box.disabled = true;
    api.connectSetEnabled(box.checked)
      .catch(error => notify({ tone: 'error', text: errorText(error) }))
      .finally(() => {
        box.disabled = false;
        void loadConnect().then(() => refreshPin().then(() => void loadConnect()));
      });
  });
  return box;
}
