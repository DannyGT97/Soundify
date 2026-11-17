// A made-up computer for the explorer: two disks, the usual folders and a few
// files, so `npm run dev` and Playwright can walk it. The real one is `core::browse`.

import type { BrowseEntry, BrowseRoots } from '../types';
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

const tree: Record<string, Node> = {
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

const delay = (ms: number) => new Promise(resolve => window.setTimeout(resolve, ms));

export const mockBrowse = {
  browseRoots: async (): Promise<BrowseRoots> => ({
    places: [
      { id: 'music', kind: 'place', path: 'C:\\Users\\Ana\\Music', totalBytes: 0, freeBytes: 0 },
      { id: 'downloads', kind: 'place', path: 'C:\\Users\\Ana\\Downloads', totalBytes: 0, freeBytes: 0 },
      { id: 'pictures', kind: 'place', path: 'C:\\Users\\Ana\\Pictures', totalBytes: 0, freeBytes: 0 },
      { id: 'desktop', kind: 'place', path: 'C:\\Users\\Ana\\Desktop', totalBytes: 0, freeBytes: 0 },
    ],
    drives: [
      { id: 'drive:C:\\', kind: 'drive', path: 'C:\\', totalBytes: 476e9, freeBytes: 121e9 },
      { id: 'drive:D:\\', kind: 'drive', path: 'D:\\', totalBytes: 931e9, freeBytes: 402e9 },
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

