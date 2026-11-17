// A made-up computer for the explorer: two disks, the usual folders and a few
// files, so `npm run dev` and Playwright can walk it. The real one is `core::browse`.

import type { BrowseEntry, BrowseRoots, ExplorerMemory, FindResult, PlaylistHit } from '../types';
import { onAndroid } from '../../platform';
import { joinPath, splitPath, validFolderName } from '../../explorerLogic';

interface Node {
  folder: boolean;
  size?: number;
  modified?: string;
  playlist?: { title: string; format: string };
  /** The folder cannot be read. */
  blocked?: boolean;
  children?: Record<string, Node>;
}

const dir = (children: Record<string, Node> = {}, extra: Partial<Node> = {}): Node => ({ folder: true, children, modified: '2026-10-02T18:00:00', ...extra });
const file = (size: number, modified = '2026-09-14T10:30:00'): Node => ({ folder: false, size, modified });
const playlist = (title: string, format: string, songs: number): Node =>
  dir({
    'soundify-playlist.json': file(1800),
    ...Object.fromEntries(Array.from({ length: songs }, (_, i) => [`${String(i + 1).padStart(3, '0')} - Canción ${i + 1}.${format.toLowerCase()}`, file(4_200_000)])),
  }, { playlist: { title, format: format.toUpperCase() } });

const phone = (): Node => dir({
  Music: dir({ Soundify: dir({ 'Lo-fi para estudiar (mp3)': playlist('Lo-fi para estudiar', 'mp3', 4) }), 'Marea baja.mp3': file(6_100_000) }),
  Download: dir({ 'cookies.txt': file(8_200, '2026-11-15T09:00:00'), 'portada-verano.png': file(820_000, '2026-11-16T20:15:00') }),
  Pictures: dir({ Portadas: dir({ 'atardecer.jpg': file(1_300_000), 'vinilo.png': file(2_100_000) }) }),
  DCIM: dir({ Camera: dir({ 'IMG_0001.jpg': file(3_000_000) }) }),
  Android: dir({ data: dir({}, { blocked: true }) }),
});

const tree: Record<string, Node> = {
  '/': dir({
    storage: dir({
      emulated: dir({ '0': phone() }),
      '3A1F-0C22': dir({ Music: dir({ 'Bandas sonoras (flac)': playlist('Bandas sonoras', 'flac', 2) }), DCIM: dir({}) }),
    }),
  }),
  'C:\\': dir({
    Users: dir({
      Ana: dir({
        Music: dir({ 'Rock de los 80 (mp3)': playlist('Rock de los 80', 'mp3', 3), 'Marea baja.mp3': file(6_100_000) }),
        Downloads: dir({
          'cookies.txt': file(8_200, '2026-11-15T09:00:00'),
          'portada-verano.png': file(820_000, '2026-11-16T20:15:00'),
          'factura.pdf': file(220_000),
        }),
        Pictures: dir({
          Portadas: dir({ 'atardecer.jpg': file(1_300_000), 'fondo-azul.jpg': file(540_000), 'vinilo.png': file(2_100_000) }),
          'IMG_0012.jpg': file(3_000_000),
        }),
        Desktop: dir({ 'notas.txt': file(2_200) }),
      }),
    }),
    Windows: dir({}, { blocked: true }),
  }),
  'D:\\': dir({
    'Música': dir({
      Soundify: dir({ 'Lo-fi para estudiar (mp3)': playlist('Lo-fi para estudiar', 'mp3', 4), 'Bandas sonoras (flac)': playlist('Bandas sonoras', 'flac', 2) }),
      Descargas: dir(),
    }),
    Copias: dir(),
  }),
};

function nodeAt(path: string): Node | null {
  const { root, parts } = splitPath(path);
  let node: Node | undefined = tree[root];
  for (const part of parts) node = node?.children?.[part];
  return node ?? null;
}

let memory: ExplorerMemory = { favorites: [], recents: [] };

function search(path: string, node: Node, depth: number, hits: PlaylistHit[]): void {
  for (const [name, child] of Object.entries(node.children ?? {})) {
    if (!child.folder || child.blocked) continue;
    const here = joinPath(path, name);
    if (child.playlist) hits.push({ path: here, title: child.playlist.title, format: child.playlist.format, depth: depth + 1 });
    else if (depth + 1 < 4) search(here, child, depth + 1, hits);
  }
}

const delay = (ms: number) => new Promise(resolve => window.setTimeout(resolve, ms));

export const mockBrowse = {
  browseRoots: async (): Promise<BrowseRoots> => onAndroid ? ({
    places: [
      { id: 'music', kind: 'place', path: '/storage/emulated/0/Music', totalBytes: 0, freeBytes: 0, readOnly: false },
      { id: 'downloads', kind: 'place', path: '/storage/emulated/0/Download', totalBytes: 0, freeBytes: 0, readOnly: false },
      { id: 'pictures', kind: 'place', path: '/storage/emulated/0/Pictures', totalBytes: 0, freeBytes: 0, readOnly: false },
      { id: 'camera', kind: 'place', path: '/storage/emulated/0/DCIM', totalBytes: 0, freeBytes: 0, readOnly: false },
    ],
    drives: [
      { id: 'internal', kind: 'drive', path: '/storage/emulated/0', totalBytes: 256e9, freeBytes: 88e9, readOnly: false },
      { id: 'sd:3A1F-0C22', kind: 'drive', path: '/storage/3A1F-0C22', totalBytes: 64e9, freeBytes: 41e9, readOnly: true },
    ],
  }) : ({
    places: [
      { id: 'music', kind: 'place', path: 'C:\\Users\\Ana\\Music', totalBytes: 0, freeBytes: 0, readOnly: false },
      { id: 'downloads', kind: 'place', path: 'C:\\Users\\Ana\\Downloads', totalBytes: 0, freeBytes: 0, readOnly: false },
      { id: 'pictures', kind: 'place', path: 'C:\\Users\\Ana\\Pictures', totalBytes: 0, freeBytes: 0, readOnly: false },
      { id: 'desktop', kind: 'place', path: 'C:\\Users\\Ana\\Desktop', totalBytes: 0, freeBytes: 0, readOnly: false },
    ],
    drives: [
      { id: 'drive:C:\\', kind: 'drive', path: 'C:\\', totalBytes: 476e9, freeBytes: 121e9, readOnly: false },
      { id: 'drive:D:\\', kind: 'drive', path: 'D:\\', totalBytes: 931e9, freeBytes: 402e9, readOnly: false },
    ],
  }),
  browseList: async (path: string): Promise<BrowseEntry[]> => {
    await delay(60);
    const node = nodeAt(path);
    if (!node) throw { key: 'explorer.error.gone', args: [] };
    if (node.blocked) throw { key: 'explorer.error.blocked', args: [] };
    return Object.entries(node.children ?? {}).map(([name, child]) => ({
      name, isFolder: child.folder, sizeBytes: child.size ?? 0, modified: child.modified ?? null, playlist: child.playlist ?? null,
    }));
  },
  browseFindPlaylists: async (path: string): Promise<FindResult> => {
    await delay(300);
    const node = nodeAt(path);
    const hits: PlaylistHit[] = [];
    if (node) search(path, node, 0, hits);
    return { hits: hits.sort((a, b) => a.path.localeCompare(b.path)), truncated: false };
  },
  explorerMemory: async (): Promise<ExplorerMemory> => ({ favorites: [...memory.favorites], recents: [...memory.recents] }),
  saveExplorerMemory: async (next: ExplorerMemory): Promise<void> => { memory = { favorites: [...next.favorites], recents: [...next.recents] }; },
  browseMkdir: async (parent: string, name: string): Promise<string> => {
    const node = nodeAt(parent);
    if (!validFolderName(name)) throw { key: 'explorer.newFolder.invalid', args: [] };
    if (!node?.children) throw { key: 'explorer.newFolder.failed', args: [] };
    const clean = name.trim();
    if (Object.keys(node.children).some(existing => existing.toLowerCase() === clean.toLowerCase())) throw { key: 'explorer.newFolder.exists', args: [] };
    node.children[clean] = dir({}, { modified: '2026-11-17T09:00:00' });
    return joinPath(parent, clean);
  },
  allowPickedFile: async (): Promise<void> => {},
};

