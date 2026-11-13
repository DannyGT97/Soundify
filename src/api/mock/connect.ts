// Soundify Connect, simulated for the browser and the tests: two devices on a network
// that is not there. The «Salón» one is paired with the PIN 123456 and, once controlled,
// behaves like a player (it answers commands with a new state). `window.__connect` lets a
// test play the other side: say what this device published and ask something of it.

import type { Api } from '..';
import type { ConnectCommand, ConnectDevice, ConnectIncoming, ConnectPlayerState, ConnectStatus } from '../types';

type ConnectApi = Pick<Api,
  | 'connectStatus' | 'connectSetEnabled' | 'connectSetName' | 'connectDevices' | 'connectAdd' | 'connectPin' | 'connectRenewPin'
  | 'connectPair' | 'connectForget' | 'connectForgetController' | 'connectSelect' | 'connectDeselect' | 'connectSend' | 'connectPublish'
  | 'onConnectDevices' | 'onConnectRemoteState' | 'onConnectRemoteClosed' | 'onConnectCommand'>;

const PIN_OF_OTHERS = '123456';

let enabled = typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockConnect');
let name = 'Este equipo';
let selected: string | null = null;
const devices: ConnectDevice[] = [
  { id: 'tv', name: 'Salón', kind: 'desktop', address: '192.168.1.20:48650', paired: false, online: true, connected: false },
  { id: 'tablet', name: 'Tablet de la cocina', kind: 'tablet', address: '192.168.1.31:48650', paired: false, online: true, connected: false },
];
let controllers: Array<{ id: string; name: string }> = [];

let onDevices: () => void = () => {};
let onRemoteState: (deviceId: string, state: ConnectPlayerState) => void = () => {};
let onRemoteClosed: (deviceId: string) => void = () => {};
let onCommand: (incoming: ConnectIncoming) => void = () => {};

/** What the controlled device is doing. */
let remote: ConnectPlayerState = emptyRemote();
/** What this device published (for the tests to read). */
const published: ConnectPlayerState[] = [];

function emptyRemote(): ConnectPlayerState {
  return {
    nowPlaying: { videoId: 'remote-1', title: 'Canción del salón', playlist: 'Mix', format: 'MP3', video: false },
    playing: true, position: 10, duration: 200, volume: 0.5, shuffle: false, repeat: 'off',
  };
}

function failure(key: string, args: string[] = []): never {
  throw { key, args };
}

function change(): void {
  for (const device of devices) device.connected = device.id === selected;
  onDevices();
}

function apply(command: ConnectCommand): void {
  switch (command.cmd) {
    case 'play': remote.playing = true; break;
    case 'pause': remote.playing = false; break;
    case 'toggle': remote.playing = !remote.playing; break;
    case 'next': remote.nowPlaying = { ...(remote.nowPlaying ?? emptyRemote().nowPlaying!), videoId: 'remote-2', title: 'Siguiente del salón' }; remote.position = 0; break;
    case 'previous': remote.position = 0; break;
    case 'seek': remote.position = command.seconds; break;
    case 'volume': remote.volume = command.volume; break;
    case 'shuffle': remote.shuffle = command.on ?? !remote.shuffle; break;
    case 'repeat': remote.repeat = command.mode ?? (remote.repeat === 'off' ? 'all' : remote.repeat === 'all' ? 'one' : 'off'); break;
    case 'load': remote.nowPlaying = { videoId: command.queue[command.index]?.videoId ?? 'x', title: command.queue[command.index]?.title ?? '', playlist: command.playlist, format: 'MP3', video: false }; remote.position = command.position; remote.playing = command.autoplay; break;
  }
}

if (typeof window !== 'undefined') {
  (window as unknown as { __connect: unknown }).__connect = {
    published,
    /** The other side asks this device for something. */
    incoming: (command: ConnectCommand) => onCommand({ from: 'Móvil de prueba', command }),
    /** The device being controlled drops. */
    drop: () => { const id = selected; selected = null; change(); if (id) onRemoteClosed(id); },
  };
}

export const mockConnect: ConnectApi = {
  connectStatus: async (): Promise<ConnectStatus> => ({
    enabled, running: enabled, port: enabled ? 48650 : null, name, deviceId: 'this-device', selected, controllers, connectedControllers: 0,
  }),
  connectSetEnabled: async value => {
    enabled = value;
    if (!enabled) selected = null;
    change();
  },
  connectSetName: async value => { name = value.trim() || name; },
  connectDevices: async () => (enabled ? devices.map(device => ({ ...device })) : []),
  connectAdd: async address => {
    if (!/^\d+\.\d+\.\d+\.\d+(:\d+)?$/.test(address.trim())) failure('connect.error.address');
    if (!devices.some(device => device.address?.startsWith(address.trim()))) failure('connect.error.offline');
    change();
  },
  connectPin: async () => (enabled ? '482915' : null),
  connectRenewPin: async () => (enabled ? '730416' : null),
  connectPair: async (deviceId, pin) => {
    const device = devices.find(d => d.id === deviceId);
    if (!device?.online) failure('connect.error.offline');
    if (pin.trim() !== PIN_OF_OTHERS) failure('connect.error.wrongPin');
    device.paired = true;
    change();
  },
  connectForget: async deviceId => {
    const device = devices.find(d => d.id === deviceId);
    if (device) device.paired = false;
    if (selected === deviceId) selected = null;
    change();
  },
  connectForgetController: async id => { controllers = controllers.filter(c => c.id !== id); change(); },
  connectSelect: async deviceId => {
    const device = devices.find(d => d.id === deviceId);
    if (!device?.paired) failure('connect.error.notPaired');
    selected = deviceId;
    remote = emptyRemote();
    change();
    window.setTimeout(() => onRemoteState(deviceId, { ...remote }), 0);
  },
  connectDeselect: async () => { selected = null; change(); },
  connectSend: async command => {
    if (!selected) failure('connect.error.noSession');
    apply(command);
    const id = selected;
    window.setTimeout(() => onRemoteState(id, { ...remote }), 0);
  },
  connectPublish: state => { published.push(state); },
  onConnectDevices: listener => { onDevices = listener; },
  onConnectRemoteState: listener => { onRemoteState = listener; },
  onConnectRemoteClosed: listener => { onRemoteClosed = listener; },
  onConnectCommand: listener => { onCommand = listener; },
};
