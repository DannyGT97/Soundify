// The player's actions: queue, shuffle and repeat. What actually sounds is the
// PlayerBackend behind `api` (a clock in the mock); these actions stay the
// same when the real engine comes in.

import { playerBackend as engine, type Track } from './api';
import { findBlock, onDisk } from './library';
import { emit, ui } from './state';

/** What the built-in player can open: by format NAME (ALAC is .m4a too and does not play). */
export const PLAYABLE = new Set(['MP3', 'M4A', 'WAV', 'MP4']);

engine.onTime(seconds => {
  ui.position = seconds;
  emit('player');
});
engine.onEnded(() => {
  if (ui.repeat === 'one') {
    seek(0);
    setPlaying(true);
  } else {
    step(1, true);
  }
});

export function play(blockKey: string, track: Track): void {
  ui.nowPlaying = { blockKey, track };
  ui.position = 0;
  engine.load(blockKey, track);
  setPlaying(true);
}

export function playAll(blockKey: string): void {
  const queue = queueOf(blockKey);
  const first = ui.shuffle ? queue[Math.floor(Math.random() * queue.length)] : queue[0];
  if (first) play(blockKey, first);
}

export function togglePlay(): void {
  if (ui.nowPlaying) setPlaying(!ui.playing);
}

export function next(): void {
  step(1);
}

export function previous(): void {
  // Like every player: past the first seconds, "previous" restarts the file.
  if (ui.position > 3) seek(0);
  else step(-1);
}

export function seek(seconds: number): void {
  ui.position = seconds;
  engine.seek(seconds);
  emit('player');
}

export function toggleShuffle(): void {
  ui.shuffle = !ui.shuffle;
  emit('player');
}

export function cycleRepeat(): void {
  ui.repeat = ui.repeat === 'off' ? 'all' : ui.repeat === 'all' ? 'one' : 'off';
  emit('player');
}

export function setVolume(volume: number): void {
  ui.volume = volume;
  engine.setVolume(volume);
  emit('player');
}

export function close(): void {
  engine.unload();
  ui.playing = false;
  ui.nowPlaying = null;
  ui.position = 0;
  emit('player');
}

function setPlaying(playing: boolean): void {
  ui.playing = playing;
  if (playing) engine.play();
  else engine.pause();
  emit('player');
}

function step(direction: 1 | -1, auto = false): void {
  const np = ui.nowPlaying;
  if (!np) return;
  const queue = queueOf(np.blockKey);
  const index = queue.findIndex(tr => tr.id === np.track.id);
  let target = ui.shuffle && direction === 1
      ? Math.floor(Math.random() * queue.length)
      : index + direction;
  if (target >= queue.length) {
    if (auto && ui.repeat === 'off') {
      setPlaying(false);
      seek(0);
      return;
    }
    target = 0;
  }
  if (target < 0) target = queue.length - 1;
  const track = queue[target];
  if (track) play(np.blockKey, track);
}

function queueOf(blockKey: string): Track[] {
  return onDisk(findBlock(blockKey)?.block.tracks ?? []);
}

/** "3:07", or "1:02:45" past the hour. */
export function formatTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(s / 3600);
  const minutes = Math.floor((s % 3600) / 60);
  const secs = String(s % 60).padStart(2, '0');
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${secs}` : `${minutes}:${secs}`;
}
