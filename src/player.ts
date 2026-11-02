// PLACEHOLDER PLAYBACK: moves the clock and the queue so the bar and Now
// Playing can be seen working. Nothing sounds; the real engine (an <audio>
// element or a Rust backend) replaces the timer, the actions stay the same.

import { findBlock, onDisk, type Track } from './mock';
import { emit, ui } from './state';

let timer: number | undefined;

export function play(blockKey: string, track: Track): void {
  ui.nowPlaying = { blockKey, track };
  ui.position = 0;
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
  emit('player');
}

export function close(): void {
  setPlaying(false);
  ui.nowPlaying = null;
  ui.position = 0;
  emit('player');
}

function setPlaying(playing: boolean): void {
  ui.playing = playing;
  window.clearInterval(timer);
  if (playing) timer = window.setInterval(tick, 1000);
  emit('player');
}

function tick(): void {
  const np = ui.nowPlaying;
  if (!np) return;
  ui.position += 1;
  if (ui.position >= np.track.seconds) {
    if (ui.repeat === 'one') ui.position = 0;
    else step(1, true);
  }
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
      ui.position = 0;
      emit('player');
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
