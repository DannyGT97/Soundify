// Every icon of the app, from Lucide (https://lucide.dev, ISC licence). They are
// SVG drawn with `currentColor`: sharp at any scale and in the colour of the
// text around them, so both themes work without a second set of files.
// Each one is imported by name, so only the ones listed here end up in the build.

import {
  Activity, AudioLines, Check, ChevronRight, Clock, ChevronsLeft, ChevronsRight, CloudDownload, Ellipsis,
  Copy, Eye, Maximize2, Minus, PictureInPicture2, Film, Folder, FolderInput, FolderOpen, House, Info, Link, List, ListMusic, ListOrdered, ListPlus, Menu, Palette, PanelLeftClose, Trash2, TriangleAlert, CircleX,
  Maximize, Minimize, Pause, Play, Wrench, RefreshCw, Repeat, Repeat1, Search, Settings, Shuffle, SkipBack, SkipForward,
  SlidersHorizontal, Smartphone, Square, Volume2, VolumeX, X, ImagePlus, Image, Unplug, FileJson, FileMusic, ChevronDown, ArrowDownToLine, Music, ArrowLeft, ArrowUp, FolderPlus, LibraryBig, Pencil, Usb, LayoutGrid, LoaderCircle, createElement, type IconNode,
} from 'lucide';

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



