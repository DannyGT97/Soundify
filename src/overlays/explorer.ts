import { h } from '../dom';
import { icon, type IconName } from '../icons';
import { t } from '../i18n';
import { api, type BrowseRoots, type DeviceInfo, type DeviceTarget, type ExplorerMemory, type FormatBlock, type Playlist, type PlaylistHit } from '../api';
import { closeButton, closeOverlay, openPanel } from '../components/overlay';
import { openFloating, closeFloating } from '../components/floating';
import { cover } from '../components/cover';
import { notify } from '../components/toasts';
import { playlistPicture } from '../covers';
import { ago } from '../relativeTime';
import { MANIFEST_NAME, errorText } from '../transfer';
import { onAndroid } from '../platform';
import { grantStorage, needsStorageAccess } from '../storage';
import {
  IMAGE_EXTENSIONS, crumbsOf, daysSince, driveFor, driveLetter, extensionOf, filterEntries, hasName, joinPath, kindOf, parentPath, playlistFolderName, playlistFromName,
  remember, samePath, sizeText, sortEntries, toggled, validFolderName, type Crumb, type ExplorerEntry, type SortBy,
} from '../explorerLogic';

/** What the explorer is opened for: it changes the words, what can be picked and where it starts. */
export type ExplorerPurpose = 'destination' | 'copy-folder' | 'copy-device' | 'import-folder' | 'import-device' | 'cover' | 'cookies';

/** A folder or a file of this computer, a folder of a phone, or (importing, after «Buscar en subcarpetas») several playlist folders. */
export type ExplorerPick = { path: string } | { device: DeviceTarget } | { paths: string[] };

/** What «Copiar» is about to copy: shown in the head and in the footer. */
export interface PickerSubject {
  playlist: Playlist;
  block: FormatBlock;
}

export interface ExplorerOptions {
  /** The head's title when the purpose's own is not specific enough ("¿Dónde quieres copiar …?"). */
  title?: string;
  subject?: PickerSubject;
  /** Starts on the first phone (the destination's «Dispositivo MTP» card). */
  startOnDevice?: boolean;
  /** The window closed without picking anything. */
  onCancel?: () => void;
}

interface Spec {
  /** A folder is chosen (true) or a file (false). */
  folder: boolean;
  local: boolean;
  device: boolean;
  /** The files that can be chosen; the others are hidden until «Mostrar todos». */
  accept?: (name: string) => boolean;
  /** Folders for what is there to see, not to choose (importing looks at what a folder holds). */
  scan?: boolean;
  icon: IconName;
  /** The i18n prefix: `explorer.<key>.title|sub|action|why`. */
  key: string;
}

const SPECS: Record<ExplorerPurpose, Spec> = {
  destination: { folder: true, local: true, device: true, icon: 'arrow-down', key: 'destination' },
  'copy-folder': { folder: true, local: true, device: false, icon: 'copy', key: 'copyFolder' },
  'copy-device': { folder: true, local: false, device: true, icon: 'copy', key: 'copyDevice' },
  'import-folder': { folder: true, local: true, device: false, scan: true, icon: 'import', key: 'importFolder' },
  'import-device': { folder: true, local: false, device: true, scan: true, icon: 'import', key: 'importDevice' },
  cover: { folder: false, local: true, device: false, accept: name => IMAGE_EXTENSIONS.includes(extensionOf(name)), icon: 'image', key: 'cover' },
  cookies: { folder: false, local: true, device: false, accept: name => extensionOf(name) === 'txt', icon: 'file-text', key: 'cookies' },
};

/** How often the list of phones is asked again, so plugging one in shows up without reopening. */
const POLL_MS = 4000;
/** Phones answer slowly over USB: past this, a listing says it is still working. */
const SLOW_MS = 1400;

const PLACE_ICONS: Record<string, IconName> = { music: 'music', downloads: 'arrow-down', pictures: 'image', camera: 'image', desktop: 'monitor' };

/** Folders starred and chosen last, shared by every window and kept in settings.json. */
let memory: ExplorerMemory | null = null;
const RECENT_MAX = 6;

async function loadMemory(): Promise<void> {
  if (memory) return;
  memory = await api.explorerMemory().catch(() => ({ favorites: [], recents: [] }));
}

function saveMemory(next: ExplorerMemory): void {
  memory = next;
  void api.saveExplorerMemory(next).catch(() => {});
}

/** The last name in a path, or the path itself at a root. */
function folderLabel(path: string): string {
  const crumbs = crumbsOf(path);
  return crumbs.length > 1 ? (crumbs[crumbs.length - 1] as { label: string }).label : path;
}

// What was left the last time stays for the next: the way of looking, and where each purpose was.
let view: 'grid' | 'list' = 'list';
let sortBy: SortBy = 'name';
type Place = { kind: 'local'; path: string } | { kind: 'device'; id: string; path: string[] };
const lastPlace: Partial<Record<ExplorerPurpose, Place>> = {};

/** "Pixel 8 / Music/Soundify", or "(raíz)" at the root: what a chosen phone folder reads as. */
export function deviceLabel(target: DeviceTarget): string {
  return t('download.deviceChosen', target.deviceName, target.path || t('explorer.root'));
}

/**
 * The file explorer: this computer's disks and folders and the phones connected
 * over USB in one window. `onPicked` gets a path of this computer or a spot
 * inside a phone (`path` "/"-joined, "" = its root).
 */
export function openExplorer(purpose: ExplorerPurpose, onPicked: (pick: ExplorerPick) => void, options: ExplorerOptions = {}): void {
  const spec = SPECS[purpose];
  // Phones over USB (MTP) are a desktop thing: a phone browses its own storage.
  const deviceOn = spec.device && !onAndroid;
  const key = (part: string, ...args: (string | number)[]) => t(`explorer.${spec.key}.${part}`, ...args);

  let roots: BrowseRoots = { places: [], drives: [] };
  let devices: DeviceInfo[] = [];
  let storages: string[] = [];
  let place: Place | null = null;
  let entries: ExplorerEntry[] | null = null;
  let failure: string | null = null;
  let lost: DeviceInfo | null = null;
  let filter = '';
  let helpOpen = false;
  let creating = false;
  let showAll = false;
  let selected: string | null = null;
  let cursor = -1;
  let searched = false;
  // Each listing gets a number: the answer for a folder already left is dropped.
  let request = 0;
  // While a phone's folder is being listed it is busy: the poll waits its turn.
  let listing = false;
  let slowTimer = 0;
  let slow = false;
  let closed = false;
  let picked = false;
  // «Buscar en subcarpetas»: the playlists found below a folder, to import several at once.
  let scan: { root: string; hits: PlaylistHit[] | null; truncated: boolean; chosen: Set<string> } | null = null;
  let scanRequest = 0;

  const device = (): DeviceInfo | undefined => {
    const where = place;
    return where?.kind === 'device' ? devices.find(d => d.id === where.id) : undefined;
  };

  // === THE PIECES THAT STAY ===
  const rail = h('div', { class: 'rail' });
  const up = h('button', { class: 'icon-button', title: t('explorer.up'), onclick: () => goUp() }, icon('up', 17));
  const crumbs = h('div', { class: 'crumbs' });
  const filterInput = h('input', { class: 'input', attrs: { type: 'search', placeholder: t('explorer.filter'), 'aria-label': t('explorer.filter') } });
  filterInput.addEventListener('input', () => {
    filter = filterInput.value;
    cursor = -1;
    renderContent();
  });
  const sortButton = h('button', { class: 'icon-button', title: t('explorer.sort'), onclick: () => openSortMenu() }, icon('sort', 15));
  const newFolder = h('button', { class: 'icon-button', title: t('explorer.newFolder'), onclick: () => {
    creating = true;
    renderContent();
  } }, icon('folder-plus', 16));
  const starButton = h('button', { class: 'icon-button', onclick: () => toggleFavorite() }, icon('star', 15));
  const refresh = h('button', { class: 'icon-button', title: t('explorer.refresh'), onclick: () => void load() }, icon('sync', 15));
  const gridButton = h('button', { class: 'icon-button', title: t('explorer.grid'), onclick: () => setView('grid') }, icon('grid', 15));
  const listButton = h('button', { class: 'icon-button', title: t('explorer.list'), onclick: () => setView('list') }, icon('list', 16));
  const bar = h('div', { class: 'browser-bar' },
    up, crumbs,
    h('div', { class: 'bar-tools' },
      h('div', { class: 'mini-search' }, icon('search', 13), filterInput),
      sortButton, !spec.folder || spec.scan ? null : newFolder, spec.local ? starButton : null, refresh, gridButton, listButton));
  const content = h('div', { class: 'browser-content', attrs: { tabindex: '0' } });
  const browserFoot = h('div', { class: 'browser-foot' });
  const details = h('aside', { class: 'xp-details' });
  const trail = h('b', { class: 'ellipsis' });
  const extra = h('small', { class: 'ellipsis' });
  const choose = h('button', { class: 'primary-button', onclick: () => pick() });
  const body = h('div', { class: `mtp-body${spec.folder ? '' : ' with-details'}` }, rail, h('div', { class: 'browser' }, bar, content, browserFoot), spec.folder ? null : details);

  // === WHAT IS CHOSEN ===

  function pick(chosen?: string): void {
    const verdict = judge(chosen);
    if (!place || !verdict.ok) return;
    lastPlace[purpose] = place;
    picked = true;
    if (scan) {
      const paths = [...scan.chosen];
      rememberFolder(scan.root);
      closeOverlay();
      onPicked({ paths });
      return;
    }
    if (spec.folder && place.kind === 'local') rememberFolder(chosen ? joinPath(place.path, chosen) : place.path);
    closeOverlay();
    if (place.kind === 'device') {
      const phone = device();
      if (!phone) return;
      onPicked({ device: { deviceId: phone.id, deviceName: phone.name, path: (chosen ? [...place.path, chosen] : place.path).join('/') } });
    } else {
      const name = chosen ?? (spec.folder ? undefined : selected ?? undefined);
      onPicked({ path: name ? joinPath(place.path, name) : place.path });
    }
  }

  /** The folder (or file) that would be chosen as things are, and the footer's words about it. */
  function judge(inside?: string): { ok: boolean; line: string; note: string; warn: boolean } {
    const no = (note: string) => ({ ok: false, line: '', note, warn: false });
    if (!place) return no(t('explorer.noneChosen'));
    if (failure) return no(t(failure === 'unplugged' ? 'explorer.unplugged.short' : 'explorer.error.short'));
    if (scan) {
      if (!scan.hits) return no(t('explorer.scan.searching'));
      return scan.chosen.size ? { ok: true, line: t('explorer.scan.line', scan.chosen.size), note: t('explorer.scan.note'), warn: false } : no(t('explorer.scan.pickSome'));
    }
    if (entries === null) return no(t('explorer.reading'));
    if (!spec.folder) {
      const file = entries.find(entry => !entry.isFolder && entry.name === selected);
      if (!file) return no(t(`explorer.${spec.key}.choose`));
      return { ok: true, line: file.name, note: [sizeText(file.sizeBytes), whenText(file.modified)].filter(Boolean).join(' · '), warn: false };
    }
    if (!spec.scan && driveOf(place)?.readOnly) return no(t('explorer.readOnly'));
    const here = placeText(place, inside);
    if (spec.scan) return { ok: true, line: here, note: key('why'), warn: false };
    const subject = options.subject;
    if (subject) {
      const folderName = playlistFolderName(subject.playlist.title, subject.block.format);
      const made = placeText(place, inside ? `${inside}/${folderName}` : folderName);
      const there = !inside && hasName(entries, folderName);
      return { ok: true, line: made, note: there ? t('explorer.alreadyThere') : [key('why', folderName), freeText(place)].filter(Boolean).join(' · '), warn: there };
    }
    return { ok: true, line: here, note: [key('why'), freeText(place)].filter(Boolean).join(' · '), warn: false };
  }

  function placeText(where: Place, inside?: string): string {
    if (where.kind === 'local') return inside ? joinPath(where.path, inside) : where.path;
    const phone = devices.find(d => d.id === where.id);
    return [phone?.name ?? '', ...where.path, ...(inside ? [inside] : [])].filter(Boolean).join(' › ');
  }

  /** The disk or storage a place of this computer is on. */
  function driveOf(where: Place): BrowseRoots['drives'][number] | undefined {
    return where.kind === 'local' ? driveFor(where.path, roots.drives) : undefined;
  }

  function freeText(where: Place): string {
    const drive = driveOf(where);
    return drive && drive.totalBytes > 0 ? t('explorer.free', sizeText(drive.freeBytes), sizeText(drive.totalBytes)) : '';
  }

  function whenText(iso: string | null | undefined): string {
    const days = daysSince(iso, new Date());
    if (days === null) return '';
    if (days < 30) {
      const when = ago(days);
      return t(when.key, when.amount);
    }
    return new Date(iso as string).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
  }

  // === FAVOURITES AND RECENTS ===

  const mem = (): ExplorerMemory => memory ?? { favorites: [], recents: [] };

  function isFavorite(): boolean {
    return place?.kind === 'local' && mem().favorites.some(path => samePath(path, (place as { path: string }).path));
  }

  function toggleFavorite(): void {
    if (place?.kind !== 'local') return;
    saveMemory({ ...mem(), favorites: toggled(mem().favorites, place.path) });
    renderRail();
    renderBar();
  }

  function rememberFolder(path: string): void {
    saveMemory({ ...mem(), recents: remember(mem().recents, path, RECENT_MAX) });
  }

  // === MOVING ===

  function setView(next: 'grid' | 'list'): void {
    view = next;
    renderBar();
    renderContent();
  }

  function openSortMenu(): void {
    const options: SortBy[] = ['name', 'date', 'size'];
    openFloating(sortButton, options.map(by => h('button', { class: by === sortBy ? 'on' : '', onclick: () => {
      closeFloating();
      sortBy = by;
      renderContent();
    } }, icon(by === sortBy ? 'ok' : 'sort', 15), h('span', { text: t(`explorer.sort.${by}`) }))));
  }

  function go(next: Place): void {
    place = next;
    filter = '';
    filterInput.value = '';
    creating = false;
    helpOpen = false;
    selected = null;
    cursor = -1;
    showAll = false;
    lost = null;
    scan = null;
    scanRequest++;
    renderRail();
    void load();
  }

  function goUp(): void {
    if (!place) return;
    if (place.kind === 'local') {
      const parent = parentPath(place.path);
      if (parent) go({ kind: 'local', path: parent });
    } else if (place.path.length) {
      go({ kind: 'device', id: place.id, path: place.path.slice(0, -1) });
    }
  }

  function enter(name: string): void {
    if (!place) return;
    go(place.kind === 'local' ? { kind: 'local', path: joinPath(place.path, name) } : { kind: 'device', id: place.id, path: [...place.path, name] });
  }

  async function load(): Promise<void> {
    const asked = ++request;
    if (!place) {
      renderBar();
      renderFoot();
      renderDetails();
      renderContent();
      return;
    }
    const where = place;
    if (onAndroid && where.kind === 'local' && needsStorageAccess()) {
      entries = [];
      failure = 'explorer.noaccess';
      renderBar();
      renderFoot();
      renderContent();
      return;
    }
    // Before anything is drawn: the footer must not offer the new folder with the old folder's answer.
    entries = null;
    failure = null;
    if (where.kind === 'device' && !device()) {
      entries = [];
      failure = 'unplugged';
      renderBar();
      renderFoot();
      renderContent();
      return;
    }
    slow = false;
    listing = where.kind === 'device';
    window.clearTimeout(slowTimer);
    renderBar();
    renderFoot();
    renderDetails();
    if (where.kind === 'device') {
      slowTimer = window.setTimeout(() => {
        if (asked !== request || entries !== null) return;
        slow = true;
        renderContent();
      }, SLOW_MS);
    }
    renderContent();
    try {
      const answer: ExplorerEntry[] = where.kind === 'local'
        ? await api.browseList(where.path)
        : (await api.listDeviceFolder(where.id, where.path.join('/'))).map(entry => ({ ...entry, modified: null, playlist: entry.isFolder ? playlistFromName(entry.name) : null }));
      if (asked !== request) return;
      entries = answer;
      if (where.kind === 'device' && where.path.length === 0) {
        storages = answer.filter(entry => entry.isFolder).map(entry => entry.name).sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' }));
        renderRail();
      }
    } catch (error) {
      if (asked !== request) return;
      const asKey = typeof error === 'object' && error !== null && 'key' in error ? String((error as { key: unknown }).key) : '';
      failure = asKey.startsWith('explorer.error.') ? asKey : 'explorer.error.unreadable';
      entries = [];
    } finally {
      if (asked === request) {
        listing = false;
        window.clearTimeout(slowTimer);
      }
    }
    if (asked !== request) return;
    renderBar();
    renderFoot();
    renderContent();
    renderDetails();
  }

  // === DRAWING ===

  function placeButton(iconName: IconName, label: string, small: string, space: Node | null, on: boolean, onclick: () => void): HTMLElement {
    return h('button', { class: `xp-place${on ? ' on' : ''}`, title: label, onclick },
      h('span', { class: 'xp-place-icon' }, icon(iconName, 16)),
      h('span', { class: 'min0 grow' }, h('b', { class: 'ellipsis', text: label }), small ? h('small', { class: 'ellipsis', text: small }) : null, space));
  }

  function spaceBar(totalBytes: number, freeBytes: number): HTMLElement {
    const used = totalBytes > 0 ? Math.round((1 - freeBytes / totalBytes) * 100) : 0;
    return h('div', { class: `xp-space${freeBytes < 10 * 1024 ** 3 ? ' low' : ''}` }, h('i', { attrs: { style: `width:${used}%` } }));
  }

  function renderRail(): void {
    const parts: (Node | null)[] = [];
    if (spec.local) {
      const here = place?.kind === 'local' ? place.path : null;
      if (roots.places.length) {
        parts.push(h('div', { class: 'rail-label', text: t('explorer.rail.places') }));
        for (const root of roots.places) {
          const name = root.id;
          parts.push(placeButton(PLACE_ICONS[name] ?? 'folder', t(`explorer.place.${name}`), '', null, here !== null && samePath(here, root.path), () => go({ kind: 'local', path: root.path })));
        }
      }
      const mine = (list: string[], label: string, iconName: IconName) => {
        if (!list.length) return;
        parts.push(h('div', { class: 'rail-label', text: label }));
        for (const path of list) {
          parts.push(placeButton(iconName, folderLabel(path), path, null, here !== null && samePath(here, path), () => go({ kind: 'local', path })));
        }
      };
      mine(mem().favorites, t('explorer.rail.favorites'), 'star');
      mine(mem().recents.filter(path => !mem().favorites.some(fav => samePath(fav, path))), t('explorer.rail.recents'), 'pending');
      if (roots.drives.length) {
        parts.push(h('div', { class: 'rail-label', text: t('explorer.rail.drives') }));
        for (const root of roots.drives) {
          const letter = driveLetter(root.path);
          const label = root.id === 'internal' ? t('explorer.internal') : root.id.startsWith('sd:') ? t('explorer.sd') : letter ? t('explorer.drive', letter) : root.path;
          const inside = here !== null && driveFor(here, roots.drives)?.id === root.id;
          const space = root.totalBytes > 0 ? t('explorer.free', sizeText(root.freeBytes), sizeText(root.totalBytes)) : '';
          parts.push(placeButton(root.id === 'internal' ? 'device' : 'drive', label, root.readOnly ? [space, t('explorer.readOnlyShort')].filter(Boolean).join(' · ') : space,
            root.totalBytes > 0 ? spaceBar(root.totalBytes, root.freeBytes) : null, inside, () => go({ kind: 'local', path: root.path })));
        }
      }
    }
    if (deviceOn) {
      parts.push(h('div', { class: 'rail-label', text: t('explorer.rail.devices') }));
      for (const phone of devices) {
        const on = place?.kind === 'device' && place.id === phone.id;
        parts.push(h('div', { class: `dev${on ? ' on' : ''}` },
          h('button', { class: 'dev-row', onclick: () => go({ kind: 'device', id: phone.id, path: [] }) },
            h('span', { class: 'dev-icon' }, icon('device', 18)),
            h('span', { class: 'min0 grow' },
              h('b', { class: 'ellipsis', text: phone.name }),
              h('span', { class: 'dev-status' }, h('i', { class: 'dot' }), h('span', { text: t('explorer.usb') })))),
          on && storages.length ? h('div', { class: 'storages' }, ...storages.map(name => h('button', {
            class: `storage${place?.kind === 'device' && place.path[0] === name ? ' on' : ''}`,
            title: name,
            onclick: () => go({ kind: 'device', id: phone.id, path: [name] }),
          }, icon('folder', 14), h('span', { class: 'ellipsis', text: name })))) : null));
      }
      if (lost && !devices.some(d => d.id === lost?.id)) {
        parts.push(h('div', { class: 'xp-gone' }, icon('usb', 14), h('span', { text: t('explorer.unplugged.title', lost.name) })));
      }
      if (!devices.length && !lost) parts.push(h('div', { class: 'xp-hint', text: searched ? t('explorer.rail.noDevices') : t('explorer.loading') }));
      parts.push(h('div', { class: 'rail-foot' },
        h('button', { class: 'rail-link', onclick: () => {
          helpOpen = !helpOpen;
          renderContent();
        } }, icon('info', 14), h('span', { text: t('help.deviceLimits.link') }))));
    }
    rail.replaceChildren(...parts.filter((part): part is Node => part !== null));
  }

  function renderBar(): void {
    const here = place;
    bar.hidden = !here;
    up.disabled = !here || (here.kind === 'local' ? parentPath(here.path) === null : here.path.length === 0);
    newFolder.disabled = !here || failure !== null || entries === null || scan !== null || (here.kind === 'local' && !!driveOf(here)?.readOnly);
    starButton.disabled = here?.kind !== 'local';
    starButton.classList.toggle('on', isFavorite());
    starButton.title = t(isFavorite() ? 'explorer.favorite.remove' : 'explorer.favorite.add');
    gridButton.classList.toggle('on', view === 'grid');
    listButton.classList.toggle('on', view === 'list');
    if (!here) return;
    let steps: Crumb[];
    if (here.kind === 'local') {
      steps = crumbsOf(here.path);
    } else {
      const phone = device();
      const name = phone?.name ?? lost?.name ?? '';
      steps = [{ label: name, path: '' }, ...here.path.map((part, i) => ({ label: part, path: here.path.slice(0, i + 1).join('/') }))];
    }
    crumbs.replaceChildren(...steps.flatMap((step, i) => [
      i ? icon('chevron', 12) : null,
      h('button', { text: step.label, onclick: () => {
        if (here.kind === 'local') go({ kind: 'local', path: step.path });
        else go({ kind: 'device', id: here.id, path: step.path ? step.path.split('/') : [] });
      } }),
    ]).filter((node): node is SVGElement | HTMLButtonElement => node !== null));
    crumbs.scrollLeft = crumbs.scrollWidth;
  }

  function renderFoot(): void {
    const verdict = judge();
    trail.textContent = verdict.ok ? verdict.line : verdict.note;
    trail.title = trail.textContent;
    extra.textContent = verdict.ok ? verdict.note : '';
    extra.classList.toggle('warn', verdict.warn);
    choose.textContent = scan && scan.chosen.size ? t('explorer.scan.action', scan.chosen.size) : key('action');
    choose.toggleAttribute('disabled', !verdict.ok);
  }

  function fileIcon(entry: ExplorerEntry): IconName {
    if (entry.playlist) return 'music';
    const kind = kindOf(entry);
    return ({ folder: 'folder', image: 'image', audio: 'file-music', video: 'video', text: 'file-text', file: 'file' } as const)[kind];
  }

  function entryRow(entry: ExplorerEntry, index: number): HTMLElement {
    const choosable = !entry.isFolder && !!selectable(entry);
    const isSelected = !entry.isFolder && entry.name === selected;
    const classes = `${isSelected ? ' sel' : ''}${cursor === index ? ' cur' : ''}${!entry.isFolder && !choosable ? ' dim' : ''}${entry.playlist ? ' sfy' : ''}`;
    const open = () => {
      if (entry.isFolder) {
        enter(entry.name);
      } else if (choosable) {
        selected = entry.name;
        cursor = index;
        renderContent();
        renderFoot();
        renderDetails();
      }
    };
    const use = spec.folder && entry.isFolder && !spec.scan
      ? h('button', { class: 'use', text: t('explorer.use'), onclick: event => {
        event.stopPropagation();
        pick(entry.name);
      } })
      : null;
    const sub = entry.playlist
      ? t('explorer.playlist', entry.playlist.format.toUpperCase())
      : entry.isFolder ? whenText(entry.modified) || t('explorer.folder') : [sizeText(entry.sizeBytes), whenText(entry.modified)].filter(Boolean).join(' · ');
    const label = entry.playlist ? entry.playlist.title : entry.name;
    const node = view === 'grid'
      ? h('div', { class: `tile-f${classes}`, title: entry.name, attrs: { role: 'button' }, onclick: open },
        h('span', { class: 'fi' }, icon(fileIcon(entry), 22)),
        h('div', { class: 'grow min0' },
          h('div', { class: 'nm ellipsis', text: label }),
          h('div', { class: 'sub ellipsis', text: sub })),
        use)
      : h('div', { class: `xp-row${classes}`, title: entry.name, attrs: { role: 'button' }, onclick: open },
        h('span', { class: 'fi' }, icon(fileIcon(entry), 17)),
        h('div', { class: 'grow min0' },
          h('span', { class: 'xp-name ellipsis', text: label }),
          entry.playlist && entry.playlist.title !== entry.name ? h('span', { class: 'xp-folder ellipsis', text: entry.name }) : null),
        h('span', { class: 'xp-sub', text: sub }),
        use);
    node.addEventListener('dblclick', () => {
      if (choosable) pick();
    });
    return node;
  }

  /** Whether the file can be chosen in this purpose. */
  function selectable(entry: ExplorerEntry): boolean {
    return !spec.folder && (!spec.accept || spec.accept(entry.name));
  }

  /** A name typed in the listing itself: no second window on top of this one. */
  function newFolderRow(): HTMLElement {
    const input = h('input', { class: 'input', attrs: { placeholder: t('explorer.newFolder.placeholder'), 'aria-label': t('explorer.newFolder') } });
    const create = async () => {
      const name = input.value.trim();
      if (!name || !place) {
        creating = false;
        renderContent();
        return;
      }
      if (!validFolderName(name)) {
        notify({ tone: 'error', text: t('explorer.newFolder.invalid') });
        return;
      }
      if (entries && hasName(entries, name)) {
        notify({ tone: 'error', text: t('explorer.newFolder.exists') });
        return;
      }
      input.disabled = true;
      const where = place;
      try {
        if (where.kind === 'local') {
          const made = await api.browseMkdir(where.path, name);
          go({ kind: 'local', path: made });
        } else if (await api.createDeviceFolder(where.id, [...where.path, name].join('/')).catch(() => false)) {
          go({ kind: 'device', id: where.id, path: [...where.path, name] });
        } else {
          throw { key: 'explorer.newFolder.failed', args: [name] };
        }
      } catch (error) {
        notify({ tone: 'error', text: errorText(error) });
        input.disabled = false;
        input.focus();
      }
    };
    input.addEventListener('keydown', event => {
      event.stopPropagation();
      if (event.key === 'Enter') void create();
      if (event.key === 'Escape') {
        creating = false;
        renderContent();
      }
    });
    window.setTimeout(() => input.focus());
    return h('div', { class: 'tile-f new-folder' }, h('span', { class: 'fi' }, icon('folder-plus', 22)), input);
  }

  function stateBox(iconName: IconName, title: string, text: string, ...buttons: HTMLElement[]): HTMLElement {
    return h('div', { class: 'empty' },
      h('div', { class: 'empty-mark' }, icon(iconName, 28)),
      h('b', { text: title }),
      text ? h('p', { class: 'muted', text }) : null,
      buttons.length ? h('div', { class: 'xp-actions' }, ...buttons) : null);
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
      h('b', { text: t(searched ? 'explorer.searching' : 'explorer.loading') }),
      h('p', { class: 'muted', text: t('explorer.searching.body') }),
      h('div', { class: 'steps' }, ...[1, 2, 3].map(n =>
        h('div', { class: 'step' }, h('i', { text: String(n) }), h('span', { text: t(`explorer.searching.step${n}`) }))))));
    browserFoot.hidden = true;
  }

  /** Rebuilds the list without losing the keyboard: the focused row is gone, so the list itself takes it. */
  function renderContent(): void {
    const hadFocus = content.contains(document.activeElement) || document.activeElement === content;
    paintContent();
    if (hadFocus) content.focus({ preventScroll: true });
  }

  function paintContent(): void {
    closeFloating();
    if (helpOpen) {
      renderHelp();
      return;
    }
    if (!place) {
      renderSearching();
      return;
    }
    if (place.kind === 'device' && !device()) {
      content.replaceChildren(stateBox('unplug', t('explorer.unplugged.title', lost?.name ?? ''), t('explorer.unplugged.body'),
        h('button', { class: 'primary-button', text: t('explorer.unplugged.retry'), onclick: () => void refreshDevices() })));
      browserFoot.hidden = true;
      return;
    }
    if (entries === null) {
      content.replaceChildren(
        slow ? h('div', { class: 'slow' }, h('span', { class: 'spin' }, icon('loader', 15)), h('span', { text: t('explorer.slow') })) : '',
        h('div', { class: 'tiles' }, ...Array.from({ length: 8 }, () => h('div', { class: 'sk' }))));
      browserFoot.hidden = true;
      return;
    }
    if (failure === 'explorer.noaccess') {
      content.replaceChildren(stateBox('lock', t('explorer.noaccess'), t('explorer.noaccess.body'),
        h('button', { class: 'primary-button', text: t('explorer.noaccess.grant'), onclick: () => void grantAccess() })));
      browserFoot.hidden = true;
      return;
    }
    if (failure) {
      content.replaceChildren(stateBox('lock', t(failure), t(`${failure}.body`),
        h('button', { class: 'secondary-button', text: t('explorer.up'), onclick: () => goUp() })));
      browserFoot.hidden = true;
      return;
    }
    if (scan) {
      paintScan(scan);
      return;
    }
    const shown = sortEntries(filterEntries(entries, filter), sortBy);
    const folders = shown.filter(entry => entry.isFolder);
    const allFiles = shown.filter(entry => !entry.isFolder);
    const files = spec.accept && !showAll ? allFiles.filter(entry => spec.accept?.(entry.name)) : allFiles;
    const hidden = allFiles.length - files.length;
    const parts: (Node | null)[] = [];

    if (entries.some(entry => !entry.isFolder && entry.name === MANIFEST_NAME)) {
      parts.push(spec.scan
        ? h('div', { class: 'banner' }, icon('ok', 16), h('span', { text: t('explorer.banner.importPlain') }))
        : h('div', { class: 'banner warn-b' }, icon('warning', 16), h('span', { text: t('explorer.banner.warn') })));
    } else if (spec.scan) {
      const found = entries.filter(entry => entry.playlist).length;
      parts.push(found
        ? h('div', { class: 'banner' }, icon('ok', 16), h('span', { text: t('explorer.banner.found', found) }))
        : place?.kind === 'local'
          ? h('div', { class: 'banner' }, icon('info', 16), h('span', { class: 'grow', text: t('explorer.banner.none') }),
            h('button', { class: 'secondary-button small', text: t('explorer.banner.search'), onclick: () => startScan() }))
          : h('div', { class: 'banner' }, icon('info', 16), h('span', { text: t('explorer.banner.looksDeeper') })));
    }
    if (!spec.scan && place && driveOf(place)?.readOnly) parts.push(h('div', { class: 'banner warn-b' }, icon('warning', 16), h('span', { text: t('explorer.readOnly') })));
    if (creating) parts.push(newFolderRow());

    let index = 0;
    const rows = (list: ExplorerEntry[]) => list.map(entry => entryRow(entry, index++));
    if (folders.length) parts.push(h('div', { class: view === 'grid' ? 'tiles' : 'xp-rows' }, ...rows(folders)));
    if (!folders.length && !files.length && !creating) {
      parts.push(filter.trim()
        ? h('p', { class: 'none', text: t('explorer.filter.none', filter.trim()) })
        : stateBox('folder', t('explorer.empty'), t(spec.folder ? 'explorer.empty.pick' : 'explorer.empty.none')));
    }
    if (files.length) {
      parts.push(
        h('div', { class: 'browser-group', text: t(spec.folder ? 'explorer.filesGroup.info' : 'explorer.filesGroup') }),
        h('div', { class: view === 'grid' ? 'tiles' : 'xp-rows' }, ...rows(files)));
    }
    if (hidden > 0) {
      parts.push(h('div', { class: 'browser-group xp-hidden' },
        h('span', { text: t('explorer.hidden', hidden) }),
        h('button', { class: 'xp-link', text: t('explorer.showAll'), onclick: () => {
          showAll = true;
          renderContent();
        } })));
    }
    content.replaceChildren(...parts.filter((part): part is Node => part !== null));
    browserFoot.hidden = false;
    browserFoot.textContent = t('explorer.count', folders.length, files.length);
    visibleEntries = [...folders, ...files];
    content.querySelector('.cur')?.scrollIntoView({ block: 'nearest' });
  }

  // === «BUSCAR EN SUBCARPETAS» ===

  function startScan(): void {
    if (place?.kind !== 'local') return;
    const root = place.path;
    const asked = ++scanRequest;
    scan = { root, hits: null, truncated: false, chosen: new Set() };
    renderContent();
    renderFoot();
    renderBar();
    void api.browseFindPlaylists(root).catch(() => ({ hits: [] as PlaylistHit[], truncated: false })).then(found => {
      if (asked !== scanRequest || !scan || closed) return;
      scan = { root, hits: found.hits, truncated: found.truncated, chosen: new Set(found.hits.map(hit => hit.path)) };
      renderContent();
      renderFoot();
    });
  }

  function leaveScan(): void {
    scan = null;
    scanRequest++;
    renderContent();
    renderFoot();
    renderBar();
  }

  /** Where a hit is, inside the folder that was searched. */
  function insideRoot(root: string, path: string): string {
    return path.slice(root.length).replace(/^[\\/]+/, '');
  }

  function paintScan(current: NonNullable<typeof scan>): void {
    browserFoot.hidden = false;
    visibleEntries = [];
    const back = h('button', { class: 'secondary-button small', text: t('explorer.scan.back'), onclick: () => leaveScan() });
    if (!current.hits) {
      content.replaceChildren(h('div', { class: 'slow' }, h('span', { class: 'spin' }, icon('loader', 15)), h('span', { text: t('explorer.scan.searching') })),
        h('div', { class: 'tiles' }, ...Array.from({ length: 5 }, () => h('div', { class: 'sk' }))));
      browserFoot.textContent = '';
      return;
    }
    const hits = current.hits;
    if (!hits.length) {
      content.replaceChildren(stateBox('search', t('explorer.scan.none'), t('explorer.scan.noneBody'), back));
      browserFoot.textContent = '';
      return;
    }
    const all = hits.every(hit => current.chosen.has(hit.path));
    const rows = hits.map(hit => {
      const on = current.chosen.has(hit.path);
      return h('div', { class: `xp-row sfy${on ? ' sel' : ''}`, attrs: { role: 'checkbox', 'aria-checked': String(on) }, title: hit.path, onclick: () => {
        if (on) current.chosen.delete(hit.path);
        else current.chosen.add(hit.path);
        renderContent();
        renderFoot();
      } },
      h('span', { class: `xp-check${on ? ' on' : ''}` }, on ? icon('ok', 12) : null),
      h('span', { class: 'fi' }, icon('music', 17)),
      h('div', { class: 'grow min0' },
        h('span', { class: 'xp-name ellipsis', text: hit.title }),
        h('span', { class: 'xp-folder ellipsis', text: insideRoot(current.root, hit.path) })),
      h('span', { class: 'xp-sub', text: hit.format.toUpperCase() }));
    });
    content.replaceChildren(
      h('div', { class: 'banner' }, icon('ok', 16), h('span', { class: 'grow', text: t('explorer.scan.found', hits.length) }),
        h('button', { class: 'secondary-button small', text: t(all ? 'explorer.scan.noneButton' : 'explorer.scan.allButton'), onclick: () => {
          if (all) current.chosen.clear();
          else hits.forEach(hit => current.chosen.add(hit.path));
          renderContent();
          renderFoot();
        } }), back),
      current.truncated ? h('div', { class: 'banner warn-b' }, icon('warning', 16), h('span', { text: t('explorer.scan.truncated') })) : '',
      h('div', { class: 'xp-rows' }, ...rows));
    browserFoot.textContent = t('explorer.scan.count', current.chosen.size, hits.length);
  }

  async function grantAccess(): Promise<void> {
    await grantStorage();
    roots = await api.browseRoots().catch(() => roots);
    renderRail();
    void load();
  }

  function renderDetails(): void {
    if (spec.folder) return;
    const file = entries?.find(entry => !entry.isFolder && entry.name === selected);
    if (!file || place?.kind !== 'local') {
      details.replaceChildren(h('div', { class: 'xp-details-empty' }, icon(spec.icon, 30), h('p', { class: 'muted', text: t(`explorer.${spec.key}.choose`) })));
      return;
    }
    details.replaceChildren(
      h('div', { class: 'xp-hero' }, icon(fileIcon(file), 44)),
      h('b', { class: 'xp-details-name', text: file.name }),
      h('dl', { class: 'xp-kv' },
        h('dt', { text: t('explorer.details.size' ) }), h('dd', { text: sizeText(file.sizeBytes) || '—' }),
        h('dt', { text: t('explorer.details.modified') }), h('dd', { text: whenText(file.modified) || '—' }),
        h('dt', { text: t('explorer.details.path') }), h('dd', { text: joinPath(place.path, file.name) })));
  }

  // === KEYBOARD ===
  let visibleEntries: ExplorerEntry[] = [];
  content.addEventListener('keydown', event => {
    if (event.target instanceof HTMLInputElement) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!visibleEntries.length) return;
      cursor = Math.max(0, Math.min(visibleEntries.length - 1, cursor + (event.key === 'ArrowDown' ? 1 : -1)));
      const entry = visibleEntries[cursor];
      if (entry && !entry.isFolder && selectable(entry)) {
        selected = entry.name;
        renderFoot();
        renderDetails();
      }
      renderContent();
    } else if (event.key === 'Enter') {
      const entry = visibleEntries[cursor];
      if (!entry) return;
      event.preventDefault();
      if (entry.isFolder) enter(entry.name);
      else if (selectable(entry)) {
        selected = entry.name;
        pick();
      }
    } else if (event.key === 'Backspace') {
      event.preventDefault();
      goUp();
    }
  });
  const findShortcut = (event: KeyboardEvent) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
      event.preventDefault();
      filterInput.focus();
    }
  };
  window.addEventListener('keydown', findShortcut);

  // === PHONES AND DISKS ===

  /** Asks for the phones again; when the chosen one is gone it says so, and when it is back it lists it again. */
  async function refreshDevices(): Promise<void> {
    if (!deviceOn || listing || closed) return;
    const answer = await api.listDevices().catch(() => ({ available: false, devices: [] as DeviceInfo[] }));
    if (closed) return;
    const first = !searched;
    searched = true;
    const changed = answer.devices.map(d => d.id).join('|') !== devices.map(d => d.id).join('|');
    devices = answer.devices;
    for (const phone of devices) seen.set(phone.id, phone);
    if (place?.kind === 'device') {
      const id = place.id;
      if (devices.some(d => d.id === id)) {
        if (lost?.id === id) {
          lost = null;
          failure = null;
          void load();
        }
      } else if (!lost) {
        lost = seen.get(id) ?? { id, name: '' };
        failure = 'unplugged';
        renderBar();
        renderContent();
        renderFoot();
      }
    } else if (!place && devices[0] && (!spec.local || options.startOnDevice)) {
      go({ kind: 'device', id: devices[0].id, path: [] });
    } else if (!place && first && spec.local) {
      startLocal();
    }
    if (changed || first) renderRail();
  }
  /** Every phone seen while the window was open: a name for the one that was unplugged. */
  const seen = new Map<string, DeviceInfo>();

  function startLocal(): void {
    const remembered = lastPlace[purpose];
    if (remembered?.kind === 'local') return go(remembered);
    // A folder chosen for a folder purpose starts the next one there (a file's folder says nothing about it).
    const recent = spec.folder ? mem().recents[0] : undefined;
    if (recent) return go({ kind: 'local', path: recent });
    const start = roots.places.find(root => root.id === 'music') ?? roots.places[0] ?? roots.drives[0];
    if (start) go({ kind: 'local', path: start.path });
  }

  const poll = deviceOn ? window.setInterval(() => void refreshDevices(), POLL_MS) : 0;

  // === THE WINDOW ===
  const subject = options.subject;
  const subjectChip = subject
    ? h('div', { class: 'mtp-subject' },
      cover(subject.playlist.title, subject.block.video, 22, playlistPicture(subject.playlist)),
      h('span', { class: 'ellipsis', text: `${subject.playlist.title} · ${subject.block.format.toUpperCase()}` }))
    : null;

  /** Plan B for what this computer's own dialog does better (a network place, a library): the system's. */
  const system = spec.local && !deviceOn
    ? h('button', { class: 'xp-link', text: t('explorer.system'), onclick: () => void systemDialog() })
    : null;
  async function systemDialog(): Promise<void> {
    const title = options.title ?? key('title');
    try {
      const chosen = purpose === 'cover' ? await api.pickCoverImage(title, t('cover.choose.filter'))
        : purpose === 'cookies' ? await api.pickCookiesFile(title, t('settings.cookies.fileFilter'))
          : await api.pickFolder(title);
      if (!chosen || closed) return;
      picked = true;
      closeOverlay();
      onPicked({ path: chosen });
    } catch (error) {
      notify({ tone: 'error', text: errorText(error) });
    }
  }

  renderRail();
  renderBar();
  renderFoot();
  renderContent();
  renderDetails();
  openPanel(h('div', { class: 'mtp xp' },
    h('div', { class: 'mtp-head' },
      h('span', { class: 'mtp-icon' }, icon(spec.icon, 20)),
      h('div', { class: 'grow min0' },
        h('h2', { class: 'ellipsis', text: options.title ?? key('title') }),
        h('div', { class: 'muted ellipsis', text: key('sub') })),
      subjectChip,
      closeButton()),
    body,
    h('div', { class: 'mtp-foot' },
      icon('folder', 18),
      h('div', { class: 'where-to' }, trail, extra),
      system,
      h('button', { class: 'secondary-button', text: t('common.cancel'), onclick: () => closeOverlay() }),
      choose)), () => {
    closed = true;
    if (!picked) options.onCancel?.();
    window.clearInterval(poll);
    window.clearTimeout(slowTimer);
    window.removeEventListener('keydown', findShortcut);
    closeFloating();
  });

  // The disks first (so the window never waits for a slow phone), then the phones.
  void (async () => {
    if (spec.local) {
      const [found] = await Promise.all([api.browseRoots().catch(() => ({ places: [], drives: [] })), loadMemory()]);
      roots = found;
      if (closed) return;
      renderRail();
      if (!options.startOnDevice || !deviceOn) startLocal();
    }
    void refreshDevices();
  })();
}
