// Soundify Connect, simulated for the browser and the tests: two devices on a network that is
// not there. Like the real thing by default, devices are OPEN: «Salón» is already paired and
// connected, and shows what it plays. «Tablet de la cocina» is strict: it needs the PIN 123456.
// Once controlled, «Salón» behaves like a player (it answers commands with a new state).
// `window.__connect` lets a test play the other side: say what this device published, ask
// something of it, make a device start playing, or drop the connection.

import type { Api } from '..';
import type { ConnectQueueItem, ConnectLibraryEntry, ConnectCommand, ConnectDevice, ConnectFirewall, ConnectIncoming, ConnectPlayerState, ConnectQueue, ConnectStatus } from '../types';

type ConnectApi = Pick<Api,
  | 'connectRequestLocalNetwork' | 'connectFirewallStatus' | 'connectFirewallAllow' | 'connectNetworkPrivate' | 'connectStatus' | 'connectSetEnabled' | 'connectSetName' | 'connectSetOpen' | 'connectDevices' | 'connectAdd' | 'connectPin' | 'connectRenewPin'
  | 'connectPair' | 'connectForget' | 'connectForgetController' | 'connectSelect' | 'connectDeselect' | 'connectSend' | 'connectPublish' | 'connectPublishQueue' | 'connectQueueOf'
  | 'onConnectDevices' | 'onConnectRemoteState' | 'onConnectRemoteQueue' | 'onConnectRemoteClosed' | 'onConnectCommand'
  | 'connectCast' | 'connectCastEnd' | 'connectLibraryOf' | 'connectPublishLibrary' | 'connectPublishBrowse' | 'onConnectRemoteLibrary' | 'onConnectRemoteBrowse'>;

const PIN_OF_STRICT = '123456';

let enabled = typeof location === 'undefined' || !new URLSearchParams(location.search).has('mockConnectOff');
let open = true;
let name = 'Este equipo';
let selected: string | null = null;
const devices: ConnectDevice[] = [
  { id: 'tv', name: 'Salón', kind: 'desktop', address: '192.168.1.20:48650', paired: true, online: true, ready: true, connected: false, needsPin: false, playing: false, nowPlaying: null },
  { id: 'tablet', name: 'Tablet de la cocina', kind: 'tablet', address: '192.168.1.31:48650', paired: false, online: true, ready: false, connected: false, needsPin: true, playing: false, nowPlaying: null },
];
// `?mockOldDevice` adds a device with an older Soundify (another version of Connect): listed, never connected to.
if (typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockOldDevice')) {
  devices.push({ id: 'old', name: 'Portátil viejo', kind: 'desktop', address: '192.168.1.40:48650', paired: false, online: true, ready: false, connected: false, needsPin: false, playing: false, nowPlaying: null, version: 'outdated' });
}
let controllers: Array<{ id: string; name: string }> = [];

let onDevices: () => void = () => {};
let onRemoteState: (deviceId: string, state: ConnectPlayerState) => void = () => {};
let onRemoteClosed: (deviceId: string) => void = () => {};
let onRemoteQueue: (deviceId: string, queue: ConnectQueue | null) => void = () => {};
let onCommand: (incoming: ConnectIncoming) => void = () => {};
let onRemoteBrowse: (deviceId: string, key: string, queue: ConnectQueue | null) => void = () => {};

/** «Salón» has its own library: two playlists to list and browse even when nothing plays there. */
const salonLibrary: Array<ConnectLibraryEntry & { items: ConnectQueueItem[] }> = [
  { key: 'salon:mix', title: 'Mix', format: 'MP3', video: false, count: 3, cover: 'remote-1', items: [{ videoId: 'remote-1', title: 'Canción del salón', addedDaysAgo: 0 }, { videoId: 'remote-2', title: 'Siguiente del salón', addedDaysAgo: 3 }, { videoId: 'remote-3', title: 'Tercera del salón', addedDaysAgo: 40 }] },
  // «Cocina» has a file that is not on disk there yet: it is listed (dimmed, with its state) when the playlist is browsed.
  { key: 'salon:cocina', title: 'Cocina', format: 'MP3', video: false, count: 2, cover: 'kitchen-1', items: [{ videoId: 'kitchen-1', title: 'Para cocinar', addedDaysAgo: 5 }, { videoId: 'kitchen-2', title: 'Para fregar', addedDaysAgo: 1 }, { videoId: 'kitchen-3', title: 'Aún sin bajar', status: 'pending' }] },
];
// `?mockManyPlaylists`: «Salón» has six playlists instead of two (to see the library with a mosaic).
if (typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockManyPlaylists')) {
  for (const [n, title] of ['Rock', 'Jazz', 'Gimnasio', 'Viajes'].entries()) {
    salonLibrary.push({ key: `salon:extra${n}`, title, format: 'MP3', video: false, count: 10 + n, cover: `extra${n}-1`, items: [{ videoId: `extra${n}-1`, title: `${title} 1` }] });
  }
}
/** `?mockSalonIdle` (or `__connect.salonIdle()`): «Salón» plays nothing, so no playlist is its current one. */
let salonIdle = typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockSalonIdle');
const published_libraries: ConnectLibraryEntry[][] = [];

/** What the controlled device is doing. */
let remote: ConnectPlayerState = emptyRemote(false);
if (salonIdle) remote = { ...remote, nowPlaying: null };
/** What this device published (for the tests to read). */
const published: ConnectPlayerState[] = [];
/** What this device passed to another one, and the reasons its passes ended (for the tests to read). */
const casts: Array<{ deviceId: string; playlist: string; index: number; position: number; items: Array<{ videoId: string; title: string }> }> = [];
const castEnds: string[] = [];
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
    case 'browse': {
      const entry = salonLibrary.find(e => e.key === command.key);
      window.setTimeout(() => onRemoteBrowse('tv', command.key, entry ? { playlist: entry.title, items: entry.items.map(i => ({ ...i })) } : null), 0);
      break;
    }
    case 'playFrom': {
      const entry = salonLibrary.find(e => e.key === command.key);
      if (!entry) break;
      salonIdle = false;
      remoteQueue.playlist = entry.title;
      // What plays is only what is on disk there.
      remoteQueue.items = entry.items.filter(i => !i.status).map(i => ({ ...i }));
      playItem(remoteQueue.items.findIndex(item => item.videoId === command.videoId), true);
      window.setTimeout(() => onRemoteQueue('tv', { ...remoteQueue, items: [...remoteQueue.items] }), 0);
      break;
    }
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
    casts,
    castEnds,
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
    /** «Salón» plays nothing (and says no playlist): what is left is its library. */
    salonIdle: () => { salonIdle = true; remote = { ...remote, nowPlaying: null, playing: false }; sync(); onDevices(); },
    libraries: published_libraries,
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
    if (!remote.nowPlaying && !salonIdle) remote = emptyRemote(false);
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
  connectCast: async request => {
    const device = devices.find(d => d.id === request.deviceId);
    if (!device?.paired || !device.online) failure('connect.error.offline');
    if (request.items.length === 0) failure('connect.error.nothingToPass');
    casts.push({ deviceId: request.deviceId, playlist: request.playlist, index: request.index, position: request.position, items: request.items.map(i => ({ videoId: i.videoId, title: i.title })) });
    selected = request.deviceId;
    // The other device plays what was passed, from the same file and second.
    const item = request.items[request.index]!;
    remote = {
      nowPlaying: { videoId: item.videoId, title: item.title, playlist: request.playlist, format: item.format.toUpperCase(), video: item.video },
      playing: request.playing, position: request.position, duration: item.duration || 200, volume: 0.5, shuffle: request.shuffle, repeat: request.repeat,
    };
    remoteQueue.playlist = request.playlist;
    remoteQueue.items = request.items.map(i => ({ videoId: i.videoId, title: i.title }));
    change();
    const id = request.deviceId;
    window.setTimeout(() => { onRemoteState(id, { ...remote }); onDevices(); }, 0);
  },
  connectCastEnd: async reason => {
    castEnds.push(reason);
    remote = { ...remote, playing: false };
  },
  connectPublish: state => { published.push(state); },
  connectPublishQueue: queue => { publishedQueues.push(queue); },
  connectQueueOf: async deviceId => (deviceId === 'tv' && !salonIdle ? { ...remoteQueue, items: [...remoteQueue.items] } : null),
  connectLibraryOf: async deviceId => (deviceId === 'tv' ? salonLibrary.map(({ items: _items, ...entry }) => entry) : []),
  connectPublishLibrary: playlists => { published_libraries.push(playlists); },
  connectPublishBrowse: () => {},
  onConnectRemoteLibrary: () => {},
  onConnectRemoteBrowse: listener => { onRemoteBrowse = listener; },
  onConnectRemoteQueue: listener => { onRemoteQueue = listener; },
  onConnectDevices: listener => { onDevices = listener; },
  onConnectRemoteState: listener => { onRemoteState = listener; },
  onConnectRemoteClosed: listener => { onRemoteClosed = listener; },
  onConnectCommand: listener => { onCommand = listener; },
};
