// PLACEHOLDER DATA. Views only for now: everything here stands in for what the
// archive (ArchiveStore in the Java app) will answer once the logic is ported.
// Kept in one file so replacing it later touches nothing else.

export type Status = 'ok' | 'pending' | 'failed' | 'unavailable';

export interface Track {
  id: string;
  number: number;
  title: string;
  addedDaysAgo: number;
  seconds: number;
  status: Status;
}

/** One playlist in one format: what the Gestor shows as a block and the Player as a row. */
export interface FormatBlock {
  key: string;
  playlistId: string;
  format: string;
  video: boolean;
  numbered: boolean;
  sync: boolean;
  lastSync: string;
  tracks: Track[];
}

export interface Playlist {
  id: string;
  title: string;
  folder: string;
  formats: FormatBlock[];
}

/** What the built-in player can open: by format NAME (ALAC is .m4a too and does not play). */
export const PLAYABLE = new Set(['MP3', 'M4A', 'WAV', 'MP4']);

export const AUDIO_FORMATS = ['MP3', 'OPUS', 'M4A', 'FLAC', 'AAC', 'ALAC', 'VORBIS', 'WAV'];
export const VIDEO_FORMATS = ['MP4', 'MKV', 'AVI', 'FLV', 'MOV', 'WEBM'];

const TITLES = [
  'Midnight Drive', 'Neon Rain', 'Paper Planes', 'Golden Hour', 'Static Hearts',
  'Ocean Avenue', 'Slow Motion', 'Northern Lights', 'Glass House', 'Echoes',
  'Wildfire', 'Low Tide', 'Satellite', 'Afterglow', 'Bittersweet', 'Daydream',
  'Electric Blue', 'Silver Lining', 'Night Owl', 'Runaway', 'Gravity', 'Polaroid',
];

function tracks(seed: number, count: number, broken: Partial<Record<Status, number>> = {}): Track[] {
  const list: Track[] = [];
  for (let i = 0; i < count; i++) {
    const title = TITLES[(i * 7 + seed) % TITLES.length] ?? 'Track';
    list.push({
      id: `vid${seed}${i}`,
      number: i + 1,
      title: i >= TITLES.length ? `${title} (Live)` : title,
      addedDaysAgo: (i * 3 + seed) % 90,
      seconds: 150 + ((i * 37 + seed * 11) % 140),
      status: 'ok',
    });
  }
  let cursor = count - 1;
  for (const [status, n] of Object.entries(broken) as [Status, number][]) {
    for (let k = 0; k < n && cursor >= 0; k++, cursor--) {
      const track = list[cursor];
      if (track) track.status = status;
    }
  }
  return list;
}

function block(playlistId: string, format: string, numbered: boolean, sync: boolean,
               lastSync: string, list: Track[]): FormatBlock {
  return {
    key: `${playlistId}:${format}`, playlistId, format,
    video: VIDEO_FORMATS.includes(format), numbered, sync, lastSync, tracks: list,
  };
}

export const playlists: Playlist[] = [
  {
    id: 'pl1', title: 'Chill de domingo', folder: 'D:\\Música\\Chill de domingo',
    formats: [
      block('pl1', 'MP3', true, true, '01/11/2026 21:14', tracks(1, 24, { pending: 2 })),
      block('pl1', 'FLAC', false, false, '20/10/2026 18:02', tracks(1, 18, { failed: 1 })),
    ],
  },
  {
    id: 'pl2', title: 'Rock clásico', folder: 'D:\\Música\\Rock clásico',
    formats: [block('pl2', 'M4A', true, true, '30/10/2026 10:41', tracks(2, 31, { unavailable: 1 }))],
  },
  {
    id: 'pl3', title: 'Conciertos', folder: 'E:\\Vídeos\\Conciertos',
    formats: [block('pl3', 'MP4', false, false, '12/10/2026 22:30', tracks(3, 9))],
  },
  {
    id: 'pl4', title: 'Para entrenar', folder: 'D:\\Música\\Para entrenar',
    formats: [block('pl4', 'OPUS', false, true, '28/10/2026 07:55', tracks(4, 15, { pending: 3 }))],
  },
];

export function findBlock(key: string | null | undefined): { playlist: Playlist; block: FormatBlock } | null {
  if (!key) return null;
  for (const playlist of playlists) {
    const found = playlist.formats.find(b => b.key === key);
    if (found) return { playlist, block: found };
  }
  return null;
}

export function countBy(list: Track[], status: Status): number {
  return list.filter(tr => tr.status === status).length;
}

/** Files actually on disk: what the player lists and plays. */
export function onDisk(list: Track[]): Track[] {
  return list.filter(tr => tr.status === 'ok');
}

export const activityLog = [
  '[download] Downloading playlist: Chill de domingo',
  '[youtube] Extracting URL: https://www.youtube.com/watch?v=vid10',
  '[info] vid10: Downloading 1 format(s): 251',
  '[download] Destination: D:\\Música\\Chill de domingo\\001 - Midnight Drive.webm',
  '[download] 100% of    3.42MiB in 00:00:01 at 2.31MiB/s',
  '[ExtractAudio] Destination: D:\\Música\\Chill de domingo\\001 - Midnight Drive.mp3',
  '[Metadata] Adding metadata to "001 - Midnight Drive.mp3"',
];
