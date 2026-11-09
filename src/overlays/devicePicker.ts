import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { api, type DeviceEntry, type DeviceInfo, type DeviceTarget, type FormatBlock, type Playlist } from '../api';
import { closeButton, closeOverlay, openPanel } from '../components/overlay';
import { cover } from '../components/cover';
import { notify } from '../components/toasts';
import { playlistPicture } from '../covers';

/** What the picker is for: it changes the words, never what can be browsed. */
export type DevicePurpose = 'destination' | 'import' | 'copy';

/** What «Copiar» is about to copy: shown in the head and in the trail's explanation. */
export interface PickerSubject {
  playlist: Playlist;
  block: FormatBlock;
}

/** How often the list of phones is asked again, so plugging one in shows up without reopening. */
const POLL_MS = 4000;
/** Phones answer slowly over USB: past this, a listing says it is still working. */
const SLOW_MS = 1400;
const MANIFEST = 'soundify-playlist.json';

/** "Pixel 8 / Music/Soundify", or "(raíz)" at the root: what a chosen destination reads as. */
export function deviceLabel(target: DeviceTarget): string {
  return t('download.deviceChosen', target.deviceName, target.path || t('devicePicker.root'));
}

const byName = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: 'base' });

function sizeText(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return bytes > 0 ? `${bytes} B` : '';
}

/** Soundify names a playlist's folder "Title (format)": enough to tell one without asking the phone. */
function playlistFolder(name: string): { title: string; format: string } | null {
  const match = /^(.+) \(([a-z0-9]{2,5})\)$/i.exec(name);
  return match ? { title: match[1] as string, format: (match[2] as string).toUpperCase() } : null;
}

// Grid or list stays as it was left for the next time.
let view: 'grid' | 'list' = 'grid';

/**
 * Choose a phone connected over USB (MTP) and a folder on it: the phones and
 * their top folders on the left, the current folder on the right. `path` comes
 * back "/"-joined, "" being the phone's root.
 */
export function openDevicePicker(purpose: DevicePurpose, onPicked: (target: DeviceTarget) => void, subject?: PickerSubject): void {
  let devices: DeviceInfo[] = [];
  let deviceId: string | null = null;
  let path: string[] = [];
  let roots: string[] = [];
  let entries: DeviceEntry[] | null = null;
  let filter = '';
  let helpOpen = false;
  let creating = false;
  let searched = false;
  // Each folder listing gets a number: an answer for a folder already left is dropped.
  let request = 0;
  // While a folder is being listed the phone is busy: the poll waits its turn.
  let listing = false;
  let slowTimer = 0;
  let slow = false;

  const current = (): DeviceInfo | undefined => devices.find(device => device.id === deviceId);
  const here = () => path.join('/');

  const rail = h('div', { class: 'rail' });
  const up = h('button', { class: 'icon-button', title: t('devicePicker.up'), onclick: () => go(path.slice(0, -1)) }, icon('up', 17));
  const crumbs = h('div', { class: 'crumbs' });
  const filterInput = h('input', { class: 'input', attrs: { type: 'search', placeholder: t('devicePicker.filter'), 'aria-label': t('devicePicker.filter') } });
  filterInput.addEventListener('input', () => {
    filter = filterInput.value;
    renderContent();
  });
  const newFolder = h('button', { class: 'icon-button', title: t('devicePicker.newFolder'), onclick: () => {
    creating = true;
    renderContent();
  } }, icon('folder-plus', 16));
  const refresh = h('button', { class: 'icon-button', title: t('devicePicker.refresh'), onclick: () => void loadFolder() }, icon('sync', 15));
  const gridButton = h('button', { class: 'icon-button', title: t('devicePicker.grid'), onclick: () => setView('grid') }, icon('grid', 15));
  const listButton = h('button', { class: 'icon-button', title: t('devicePicker.list'), onclick: () => setView('list') }, icon('list', 16));
  const bar = h('div', { class: 'browser-bar' },
    up, crumbs,
    h('div', { class: 'bar-tools' },
      h('div', { class: 'mini-search' }, icon('search', 13), filterInput),
      purpose === 'import' ? null : newFolder,
      refresh, gridButton, listButton));
  const content = h('div', { class: 'browser-content' });
  const browserFoot = h('div', { class: 'browser-foot' });
  const trail = h('b', { class: 'ellipsis' });
  const choose = h('button', { class: 'primary-button', text: t(`devicePicker.${purpose}.action`), onclick: () => choosePath(path) });

  function setView(next: 'grid' | 'list'): void {
    view = next;
    renderBar();
    renderContent();
  }

  function choosePath(chosen: string[]): void {
    const device = current();
    if (!device) return;
    closeOverlay();
    onPicked({ deviceId: device.id, deviceName: device.name, path: chosen.join('/') });
  }

  function go(next: string[]): void {
    path = next;
    filter = '';
    filterInput.value = '';
    creating = false;
    helpOpen = false;
    renderRail();
    void loadFolder();
  }

  function renderRail(): void {
    const help = h('div', { class: 'rail-foot' },
      h('button', { class: 'rail-link', onclick: () => {
        helpOpen = !helpOpen;
        renderContent();
      } }, icon('info', 14), h('span', { text: t('help.deviceLimits.link') })));
    rail.replaceChildren(
      h('div', { class: 'rail-label', text: t('devicePicker.rail') }),
      ...devices.map(device => {
        const on = device.id === deviceId;
        return h('div', { class: `dev${on ? ' on' : ''}` },
          h('button', { class: 'dev-row', onclick: () => pickDevice(device.id) },
            h('span', { class: 'dev-icon' }, icon('device', 18)),
            h('span', { class: 'min0 grow' },
              h('b', { class: 'ellipsis', text: device.name }),
              h('span', { class: 'dev-status' }, h('i', { class: 'dot' }), h('span', { text: t('devicePicker.usb') })))),
          on && roots.length ? h('div', { class: 'storages' }, ...roots.map(name => h('button', {
            class: `storage${path[0] === name ? ' on' : ''}`,
            title: name,
            onclick: () => go([name]),
          }, icon('folder', 14), h('span', { class: 'ellipsis', text: name })))) : null);
      }),
      help);
  }

  function renderBar(): void {
    const device = current();
    bar.hidden = !device;
    up.disabled = path.length === 0;
    newFolder.disabled = path.length === 0;
    gridButton.classList.toggle('on', view === 'grid');
    listButton.classList.toggle('on', view === 'list');
    if (device) {
      crumbs.replaceChildren(
        h('button', { text: device.name, onclick: () => go([]) }),
        ...path.flatMap((name, i) => [
          icon('chevron', 12),
          h('button', { text: name, onclick: () => go(path.slice(0, i + 1)) }),
        ]));
      crumbs.scrollLeft = crumbs.scrollWidth;
    }
  }

  function renderFoot(): void {
    const device = current();
    trail.textContent = device ? [device.name, ...path].join(' › ') : t('devicePicker.noneChosen');
    choose.disabled = !device;
  }

  function folderRow(name: string): HTMLElement {
    const known = playlistFolder(name);
    const inside = [...path, name];
    const use = h('button', { class: 'use', text: t('devicePicker.use'), onclick: event => {
      event.stopPropagation();
      choosePath(inside);
    } });
    const open = () => go(inside);
    const row = view === 'grid'
      ? h('div', { class: 'tile-f', title: name, attrs: { role: 'button', tabindex: '0' }, onclick: open },
        h('span', { class: 'fi' }, icon('folder', 22)),
        h('div', { class: 'grow min0' },
          h('div', { class: 'nm ellipsis', text: name }),
          h('div', { class: 'sub ellipsis', text: known ? t('devicePicker.playlistFolder', known.format) : t('devicePicker.folder') })),
        use)
      : h('div', { class: 'row-f dir', title: name, attrs: { role: 'button', tabindex: '0' }, onclick: open },
        h('span', { class: 'fi' }, icon('folder', 17)),
        h('span', { class: 'ellipsis', text: name }),
        h('span', { class: 'size', text: known ? known.format : '' }),
        use);
    row.addEventListener('keydown', event => {
      if (event.key === 'Enter' && event.target === row) open();
    });
    return row;
  }

  function fileRow(entry: DeviceEntry): HTMLElement {
    return h('div', { class: 'row-f file', title: entry.name },
      icon(entry.name === MANIFEST ? 'file-json' : 'file-music', 16),
      h('span', { class: 'ellipsis', text: entry.name }),
      h('span', { class: 'size', text: sizeText(entry.sizeBytes) }));
  }

  /** A name typed in the listing itself: no second window on top of this one. */
  function newFolderRow(): HTMLElement {
    const input = h('input', { class: 'input', attrs: { placeholder: t('devicePicker.newFolder.placeholder'), 'aria-label': t('devicePicker.newFolder') } });
    const create = async () => {
      const name = input.value.trim();
      const device = current();
      if (!name || !device) {
        creating = false;
        renderContent();
        return;
      }
      if (/[\\/:*?"<>|]/.test(name) || name === '.' || name === '..') {
        notify({ tone: 'error', text: t('devicePicker.newFolder.invalid') });
        return;
      }
      input.disabled = true;
      const inside = [...path, name];
      const made = await api.createDeviceFolder(device.id, inside.join('/')).catch(() => false);
      if (!made) {
        notify({ tone: 'error', text: t('devicePicker.newFolder.failed', name) });
        input.disabled = false;
        input.focus();
        return;
      }
      go(inside);
    };
    input.addEventListener('keydown', event => {
      if (event.key === 'Enter') void create();
      if (event.key === 'Escape') {
        event.stopPropagation();
        creating = false;
        renderContent();
      }
    });
    window.setTimeout(() => input.focus());
    return h('div', { class: 'tile-f new-folder' }, h('span', { class: 'fi' }, icon('folder-plus', 22)), input);
  }

  function renderHelp(): void {
    content.replaceChildren(
      h('div', { class: 'help-steps' },
        h('b', { class: 'row-title', text: t('help.deviceLimits.title') }),
        h('p', { class: 'muted', text: t('help.deviceLimits.intro') }),
        ...[1, 2, 3, 4].map(n => h('div', {}, h('i', { text: String(n) }), h('span', { text: t(`help.deviceLimits.step${n}`) }))),
        h('p', { class: 'muted', text: t('help.deviceLimits.note') })));
    browserFoot.hidden = true;
  }

  function renderSearching(): void {
    content.replaceChildren(h('div', { class: 'empty' },
      h('div', { class: 'pulse' }, icon('usb', 32)),
      h('b', { text: t(searched ? 'devicePicker.searching' : 'devicePicker.loading') }),
      h('p', { class: 'muted', text: t('devicePicker.searching.body') }),
      h('div', { class: 'steps' }, ...[1, 2, 3].map(n =>
        h('div', { class: 'step' }, h('i', { text: String(n) }), h('span', { text: t(`devicePicker.searching.step${n}`) }))))));
    browserFoot.hidden = true;
  }

  function renderContent(): void {
    if (helpOpen) {
      renderHelp();
      return;
    }
    if (!current()) {
      renderSearching();
      return;
    }
    if (entries === null) {
      content.replaceChildren(
        slow ? h('div', { class: 'slow' }, h('span', { class: 'spin' }, icon('loader', 15)), h('span', { text: t('devicePicker.slow') })) : '',
        h('div', { class: 'tiles' }, ...Array.from({ length: 8 }, () => h('div', { class: 'sk' }))));
      browserFoot.hidden = true;
      return;
    }
    const needle = filter.trim().toLowerCase();
    const shown = entries.filter(entry => entry.name.toLowerCase().includes(needle));
    const folders = shown.filter(entry => entry.isFolder).map(entry => entry.name).sort(byName);
    const files = shown.filter(entry => !entry.isFolder).sort((a, b) => byName(a.name, b.name));
    const isPlaylist = entries.some(entry => !entry.isFolder && entry.name === MANIFEST);
    const here = playlistFolder(path[path.length - 1] ?? '');

    const parts: (Node | null)[] = [];
    if (isPlaylist) {
      parts.push(purpose === 'import'
        ? h('div', { class: 'banner' }, icon('ok', 16), h('span', { text: here
          ? t('devicePicker.banner.import', here.title, here.format)
          : t('devicePicker.banner.importPlain') }))
        : h('div', { class: 'banner warn-b' }, icon('warning', 16), h('span', { text: t('devicePicker.banner.warn') })));
    }
    if (creating) parts.push(newFolderRow());
    if (folders.length) {
      parts.push(view === 'grid'
        ? h('div', { class: 'tiles' }, ...folders.map(folderRow))
        : h('div', { class: 'file-rows' }, ...folders.map(folderRow)));
    }
    if (!folders.length && !creating) {
      parts.push(needle
        ? h('p', { class: 'none', text: t('devicePicker.filter.none', filter.trim()) })
        : h('div', { class: 'empty' },
          h('div', { class: 'empty-mark' }, icon('folder', 28)),
          h('b', { text: t('devicePicker.emptyFolder') }),
          h('p', { class: 'muted', text: t('devicePicker.emptyFolder.body') })));
    }
    if (files.length) {
      parts.push(
        h('div', { class: 'browser-group', text: t('devicePicker.filesGroup') }),
        h('div', { class: 'file-rows' }, ...files.map(fileRow)));
    }
    content.replaceChildren(...parts.filter((part): part is Node => part !== null));
    browserFoot.hidden = false;
    browserFoot.textContent = t('devicePicker.count', folders.length, files.length);
  }

  async function loadFolder(): Promise<void> {
    const device = current();
    renderBar();
    renderFoot();
    if (!device) {
      renderContent();
      return;
    }
    const asked = ++request;
    listing = true;
    entries = null;
    slow = false;
    window.clearTimeout(slowTimer);
    slowTimer = window.setTimeout(() => {
      if (asked !== request || entries !== null) return;
      slow = true;
      renderContent();
    }, SLOW_MS);
    renderContent();
    const answer = await api.listDeviceFolder(device.id, here()).catch(() => []);
    if (asked !== request) return;
    listing = false;
    window.clearTimeout(slowTimer);
    entries = answer;
    if (path.length === 0) {
      roots = answer.filter(entry => entry.isFolder).map(entry => entry.name).sort(byName);
      renderRail();
    }
    renderContent();
  }

  function pickDevice(id: string): void {
    if (id === deviceId) {
      go([]);
      return;
    }
    deviceId = id;
    roots = [];
    go([]);
  }

  /** Asks for the phones again; when the chosen one is gone, the next one starts at its root. */
  async function refreshDevices(): Promise<void> {
    if (listing) return;
    const answer = await api.listDevices().catch(() => ({ available: false, devices: [] }));
    searched = true;
    const changed = answer.devices.map(device => device.id).join('|') !== devices.map(device => device.id).join('|');
    devices = answer.devices;
    if (devices.some(device => device.id === deviceId)) {
      if (changed) renderRail();
      return;
    }
    deviceId = devices[0]?.id ?? null;
    roots = [];
    path = [];
    entries = null;
    renderRail();
    void loadFolder();
  }

  const poll = window.setInterval(() => void refreshDevices(), POLL_MS);
  const subjectChip = subject ? h('div', { class: 'mtp-subject' },
    cover(subject.playlist.title, subject.block.video, 22, playlistPicture(subject.playlist)),
    h('span', { class: 'ellipsis', text: `${subject.playlist.title} · ${subject.block.format.toUpperCase()}` })) : null;
  const why = subject
    ? t('devicePicker.copy.why', `${subject.playlist.title} (${subject.block.format.toLowerCase()})`)
    : t(`devicePicker.${purpose}.why`);

  renderRail();
  renderBar();
  renderFoot();
  renderContent();
  openPanel(h('div', { class: 'mtp' },
    h('div', { class: 'mtp-head' },
      h('span', { class: 'mtp-icon' }, icon('device', 20)),
      h('div', { class: 'grow min0' },
        h('h2', { class: 'ellipsis', text: t(`devicePicker.${purpose}.title`) }),
        h('div', { class: 'muted ellipsis', text: t(`devicePicker.${purpose}.sub`) })),
      subjectChip,
      closeButton()),
    h('div', { class: 'mtp-body' },
      rail,
      h('div', { class: 'browser' }, bar, content, browserFoot)),
    h('div', { class: 'mtp-foot' },
      icon('folder', 18),
      h('div', { class: 'where-to' }, trail, h('small', { class: 'ellipsis', text: why })),
      h('button', { class: 'secondary-button', text: t('common.cancel'), onclick: () => closeOverlay() }),
      choose)), () => {
    window.clearInterval(poll);
    window.clearTimeout(slowTimer);
  });
  void refreshDevices();
}
