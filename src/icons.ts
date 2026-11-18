// Every icon of the app, from Lucide (https://lucide.dev, ISC licence). They are
// SVG drawn with `currentColor`: sharp at any scale and in the colour of the
// text around them, so both themes work without a second set of files.
// Each one is imported by name, so only the ones listed here end up in the build.

import {
  Activity, AudioLines, Check, ChevronRight, Clock, ChevronsLeft, ChevronsRight, CloudDownload, Ellipsis,
  Copy, Eye, Maximize2, Minus, PictureInPicture2, Film, Folder, FolderInput, FolderOpen, House, Info, Link, List, ListMusic, ListOrdered, ListPlus, Menu, Palette, PanelLeftClose, Trash2, TriangleAlert, CircleX,
  Maximize, Minimize, Pause, Play, Wrench, RefreshCw, Repeat, Repeat1, Search, Settings, Shuffle, SkipBack, SkipForward,
  SlidersHorizontal, Smartphone, Square, Volume2, VolumeX, X, ImagePlus, Image, Unplug, FileJson, FileMusic, ChevronDown, ArrowDownToLine, Music, ArrowLeft, ArrowUp, FolderPlus, LibraryBig, Pencil, Usb, LayoutGrid, LoaderCircle, Headphones, Bluetooth, Monitor, Bell, HardDrive, File, FileText, Lock, ArrowUpDown, Star, Cast, createElement, type IconNode,
} from 'lucide';

/** A television remote (Lucide has none): the body, its light, the round «OK» button and four more under it. Drawn like the rest. */
const REMOTE: IconNode = [
  ['rect', { x: '7', y: '2', width: '10', height: '20', rx: '3' }],
  ['path', { d: 'M12 5.5h.01' }],
  ['circle', { cx: '12', cy: '10.5', r: '2' }],
  ['path', { d: 'M10 16h.01M14 16h.01M10 19h.01M14 19h.01' }],
];

const ICONS = {
  // Menu and screens
  home: House,
  download: Link,
  'new-playlist': ListPlus,
  manager: LibraryBig,
  player: ListMusic,
  settings: Settings,
  palette: Palette,
  'panel-left-close': PanelLeftClose,
  warning: TriangleAlert,
  error: CircleX,
  info: Info,
  // Settings tabs
  general: SlidersHorizontal,
  interface: Palette,
  updates: CloudDownload,
  advanced: Settings,
  about: Info,
  tools: Wrench,
  // Actions
  import: FolderInput,
  sync: RefreshCw,
  eye: Eye,
  copy: Copy,
  'folder-open': FolderOpen,
  'list-ordered': ListOrdered,
  list: List,
  delete: Trash2,
  folder: Folder,
  device: Smartphone,
  activity: Activity,
  bell: Bell,
  search: Search,
  menu: Menu,
  more: Ellipsis,
  close: X,
  'summary-hide': ChevronsRight,
  'summary-show': ChevronsLeft,
  chevron: ChevronRight,
  // Player
  play: Play,
  pause: Pause,
  previous: SkipBack,
  next: SkipForward,
  shuffle: Shuffle,
  repeat: Repeat,
  'repeat-one': Repeat1,
  volume: Volume2,
  muted: VolumeX,
  pip: PictureInPicture2,
  'pip-expand': Maximize2,
  'window-minimize': Minus,
  'window-maximize': Square,
  'window-restore': Copy,
  fullscreen: Maximize,
  'fullscreen-exit': Minimize,
  // Results and kinds
  ok: Check,
  pending: Clock,
  stopped: Square,
  audio: AudioLines,
  video: Film,
  // Forms, library and device explorer
  'chevron-down': ChevronDown,
  'image-plus': ImagePlus,
  image: Image,
  pencil: Pencil,
  unplug: Unplug,
  'file-json': FileJson,
  'file-music': FileMusic,
  'arrow-down': ArrowDownToLine,
  music: Music,
  back: ArrowLeft,
  up: ArrowUp,
  'folder-plus': FolderPlus,
  usb: Usb,
  grid: LayoutGrid,
  loader: LoaderCircle,
  headphones: Headphones,
  bluetooth: Bluetooth,
  monitor: Monitor,
  drive: HardDrive,
  file: File,
  'file-text': FileText,
  lock: Lock,
  sort: ArrowUpDown,
  star: Star,
  // Soundify Connect: «Escuchar en» (this device's music plays on another one) and «Controlar» (driving another one's, with a remote).
  cast: Cast,
  control: REMOTE,
} satisfies Record<string, IconNode>;

export type IconName = keyof typeof ICONS;

/** An icon at `size` px; `filled` paints its inside too (play, pause, stop). */
export function icon(name: IconName, size = 18, filled = false): SVGElement {
  const svg = createElement(ICONS[name], {
    width: size,
    height: size,
    'stroke-width': size <= 16 ? 2.25 : 2,
    'aria-hidden': 'true',
    class: 'icon',
  });
  if (filled) svg.setAttribute('fill', 'currentColor');
  return svg;
}



