// PLACEHOLDER DATA: what the archive (ArchiveStore in the Java app) will answer
// once the logic is ported. Only the mock API reads it.

import type { FormatBlock, Playlist, Status, Track } from '../types';

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
      if (!track) continue;
      track.status = status;
      if (status === 'unavailable') track.reason = 'unavailable.privateVideo';
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

export const activityLog = [
  '[download] Downloading playlist: Chill de domingo',
  '[youtube] Extracting URL: https://www.youtube.com/watch?v=vid10',
  '[info] vid10: Downloading 1 format(s): 251',
  '[download] Destination: D:\\Música\\Chill de domingo\\001 - Midnight Drive.webm',
  '[download] 100% of    3.42MiB in 00:00:01 at 2.31MiB/s',
  '[ExtractAudio] Destination: D:\\Música\\Chill de domingo\\001 - Midnight Drive.mp3',
  '[Metadata] Adding metadata to "001 - Midnight Drive.mp3"',
];
