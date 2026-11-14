// Soundify Connect, simulated for the browser and the tests: two devices on a network that is
// not there. Like the real thing by default, devices are OPEN: «Salón» is already paired and
// connected, and shows what it plays. «Tablet de la cocina» is strict: it needs the PIN 123456.
// Once controlled, «Salón» behaves like a player (it answers commands with a new state).
// `window.__connect` lets a test play the other side: say what this device published, ask
// something of it, make a device start playing, or drop the connection.

import type { Api } from '..';
import type { ConnectCommand, ConnectDevice, ConnectFirewall, ConnectIncoming, ConnectPlayerState, ConnectQueue, ConnectStatus } from '../types';

type ConnectApi = Pick<Api,
  | 'connectRequestLocalNetwork' | 'connectFirewallStatus' | 'connectFirewallAllow' | 'connectNetworkPrivate' | 'connectStatus' | 'connectSetEnabled' | 'connectSetName' | 'connectSetOpen' | 'connectDevices' | 'connectAdd' | 'connectPin' | 'connectRenewPin'
  | 'connectPair' | 'connectForget' | 'connectForgetController' | 'connectSelect' | 'connectDeselect' | 'connectSend' | 'connectPublish' | 'connectPublishQueue' | 'connectQueueOf'
  | 'onConnectDevices' | 'onConnectRemoteState' | 'onConnectRemoteQueue' | 'onConnectRemoteClosed' | 'onConnectCommand'>;

const PIN_OF_STRICT = '123456';

let enabled = typeof location === 'undefined' || !new URLSearchParams(location.search).has('mockConnectOff');
let open = true;
let name = 'Este equipo';
let selected: string | null = null;
const devices: ConnectDevice[] = [
  { id: 'tv', name: 'Salón', kind: 'desktop', address: '192.168.1.20:48650', paired: true, online: true, ready: true, connected: false, needsPin: false, playing: false, nowPlaying: null },
  { id: 'tablet', name: 'Tablet de la cocina', kind: 'tablet', address: '192.168.1.31:48650', paired: false, online: true, ready: false, connected: false, needsPin: true, playing: false, nowPlaying: null },
];
let controllers: Array<{ id: string; name: string }> = [];

let onDevices: () => void = () => {};
let onRemoteState: (deviceId: string, state: ConnectPlayerState) => void = () => {};
let onRemoteClosed: (deviceId: string) => void = () => {};
let onRemoteQueue: (deviceId: string, queue: ConnectQueue | null) => void = () => {};
let onCommand: (incoming: ConnectIncoming) => void = () => {};

/** What the controlled device is doing. */
let remote: ConnectPlayerState = emptyRemote(false);
/** What this device published (for the tests to read). */
const published: ConnectPlayerState[] = [];
const publishedQueues: Array<ConnectQueue | null> = [];
/** The playlist «Salón» plays. */
const remoteQueue: ConnectQueue = {
  playlist: 'Mix',
  items: [{ videoId: 'remote-1', title: 'Canción del salón' }, { videoId: 'remote-2', title: 'Siguiente del salón' }, { videoId: 'remote-3', title: 'Tercera del salón' }],
};

function emptyRemote(playing: boolean): ConnectPlayerState {
  return {
    nowPlaying: { videoId: 'remote-1', title: 'Canción del salón', playlist: 'Mix', format: 'MP3', video: false },
    playing, position: 10, duration: 200, volume: 0.5, shuffle: false, repeat: 'off',
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
    case 'next': playItem(Math.min(remoteQueue.items.length - 1, positionInQueue() + 1), remote.playing); break;
    case 'previous': remote.position = 0; break;
    case 'seek': remote.position = command.seconds; break;
    case 'volume': remote.volume = command.volume; break;
    case 'shuffle': remote.shuffle = command.on ?? !remote.shuffle; break;
    case 'repeat': remote.repeat = command.mode ?? (remote.repeat === 'off' ? 'all' : remote.repeat === 'all' ? 'one' : 'off'); break;
    case 'take': break;
    case 'playTrack': playItem(remoteQueue.items.findIndex(item => item.videoId === command.videoId), true); break;
  }
}

function positionInQueue(): number {
  return Math.max(0, remoteQueue.items.findIndex(item => item.videoId === remote.nowPlaying?.videoId));
}

function playItem(index: number, playing: boolean): void {
  const item = remoteQueue.items[index];
  if (!item) return;
  remote.nowPlaying = { videoId: item.videoId, title: item.title, playlist: remoteQueue.playlist, format: 'MP3', video: false };
  remote.position = 0;
  remote.playing = playing;
}

/** The list line follows what the device plays. */
function sync(): void {
  const tv = devices.find(d => d.id === 'tv');
  if (tv) {
    tv.playing = remote.playing && !!remote.nowPlaying;
    tv.nowPlaying = tv.playing && remote.nowPlaying ? { title: remote.nowPlaying.title, playlist: remote.nowPlaying.playlist } : null;
  }
}

if (typeof window !== 'undefined') {
  (window as unknown as { __connect: unknown }).__connect = {
    published,
    publishedQueues,
    /** The other side asks this device for something. */
    incoming: (command: ConnectCommand) => onCommand({ from: 'Móvil de prueba', command }),
    /** The device being controlled drops. */
    drop: () => { const id = selected; selected = null; change(); if (id) onRemoteClosed(id); },
    /** «Salón» starts playing another playlist (it says so to whoever controls it). */
    salonQueue: (playlist: string, titles: string[]) => {
      remoteQueue.playlist = playlist;
      remoteQueue.items = titles.map((title, n) => ({ videoId: `other-${n + 1}`, title }));
      onRemoteQueue('tv', { ...remoteQueue, items: [...remoteQueue.items] });
    },
    /** «Salón» starts (or stops) playing on its own, as somebody pressing play there. */
    salonPlays: (playing: boolean) => { remote = emptyRemote(playing); sync(); onDevices(); },
  };
}

// `?mockFirewall` pretends to be Windows with the app not let in yet and a Public network.
// `?mockLocalNetwork` pretends to be Android 17 with the local network permission not given yet.
let localNetwork: boolean | null = typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockLocalNetwork') ? false : null;
const mockFirewall = typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockFirewall');
let firewall: ConnectFirewall = { supported: mockFirewall, allowed: false, publicNetwork: mockFirewall };

export const mockConnect: ConnectApi = {
  connectFirewallStatus: async () => ({ ...firewall }),
  connectRequestLocalNetwork: async () => { localNetwork = true; },
  connectNetworkPrivate: async () => {
    firewall = { ...firewall, publicNetwork: false };
    return { ...firewall };
  },
  connectFirewallAllow: async () => {
    firewall = { ...firewall, allowed: true };
    return { ...firewall };
  },
  connectStatus: async (): Promise<ConnectStatus> => ({
    enabled, open, addresses: enabled ? ['192.168.1.35:48650'] : [], discovery: true, error: null, localNetwork: localNetwork, running: enabled, port: enabled ? 48650 : null, name, deviceId: 'this-device', selected, controllers, connectedControllers: 0,
  }),
  connectSetEnabled: async value => {
    enabled = value;
    if (!enabled) selected = null;
    change();
  },
  connectSetName: async value => { name = value.trim() || name; },
  connectSetOpen: async value => { open = value; onDevices(); },
  connectDevices: async () => (enabled ? devices.map(device => ({ ...device })) : []),
  connectAdd: async address => {
    if (!/^\d+\.\d+\.\d+\.\d+(:\d+)?$/.test(address.trim())) failure('connect.error.address');
    if (!devices.some(device => device.address?.startsWith(address.trim()))) failure('connect.error.offline');
    change();
  },
  connectPin: async () => (enabled && !open ? '482915' : null),
  connectRenewPin: async () => (enabled && !open ? '730416' : null),
  connectPair: async (deviceId, pin) => {
    const device = devices.find(d => d.id === deviceId);
    if (!device?.online) failure('connect.error.offline');
    if (pin.trim() !== PIN_OF_STRICT) failure('connect.error.wrongPin');
    device.paired = true;
    device.ready = true;
    device.needsPin = false;
    change();
  },
  connectForget: async deviceId => {
    const device = devices.find(d => d.id === deviceId);
    if (device) { device.paired = false; device.ready = false; }
    if (selected === deviceId) selected = null;
    change();
  },
  connectForgetController: async id => { controllers = controllers.filter(c => c.id !== id); change(); },
  connectSelect: async deviceId => {
    const device = devices.find(d => d.id === deviceId);
    if (!device?.paired) failure('connect.error.notPaired');
    selected = deviceId;
    // Choosing it does not start anything by itself: what it is doing stays as it is.
    if (!remote.nowPlaying) remote = emptyRemote(false);
    change();
    return { ...remote };
  },
  connectDeselect: async () => { selected = null; change(); },
  connectSend: async command => {
    if (!selected) failure('connect.error.noSession');
    apply(command);
    sync();
    const id = selected;
    window.setTimeout(() => { onRemoteState(id, { ...remote }); onDevices(); }, 0);
  },
  connectPublish: state => { published.push(state); },
  connectPublishQueue: queue => { publishedQueues.push(queue); },
  connectQueueOf: async deviceId => (deviceId === 'tv' ? { ...remoteQueue, items: [...remoteQueue.items] } : null),
  onConnectRemoteQueue: listener => { onRemoteQueue = listener; },
  onConnectDevices: listener => { onDevices = listener; },
  onConnectRemoteState: listener => { onRemoteState = listener; },
  onConnectRemoteClosed: listener => { onRemoteClosed = listener; },
  onConnectCommand: listener => { onCommand = listener; },
};
