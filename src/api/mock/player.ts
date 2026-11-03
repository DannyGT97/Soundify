// PLACEHOLDER ENGINE: a clock that moves one second per second, so the bar and
// Now Playing can be seen working. Nothing sounds. It ends a file when the
// clock reaches the track's length, like a real engine would.

import type { PlayerBackend, Track } from '../types';

export class SimulatedPlayer implements PlayerBackend {
  private timer: number | undefined;
  private position = 0;
  private duration = 0;
  private timeListeners: ((seconds: number) => void)[] = [];
  private endListeners: (() => void)[] = [];
  private durationListeners: ((seconds: number) => void)[] = [];

  load(_blockKey: string, track: Track): void {
    this.pause();
    this.position = 0;
    this.duration = track.seconds;
    // A real engine needs a moment to open the file: never in the same call.
    window.setTimeout(() => this.durationListeners.forEach(listener => listener(this.duration)), 0);
  }

  play(): void {
    window.clearInterval(this.timer);
    this.timer = window.setInterval(() => this.tick(), 1000);
  }

  pause(): void {
    window.clearInterval(this.timer);
    this.timer = undefined;
  }

  seek(seconds: number): void {
    this.position = seconds;
  }

  setVolume(_volume: number): void {
    // Nothing sounds, so there is nothing to turn up or down.
  }

  unload(): void {
    this.pause();
    this.position = 0;
    this.duration = 0;
  }

  onTime(listener: (seconds: number) => void): void {
    this.timeListeners.push(listener);
  }

  onEnded(listener: () => void): void {
    this.endListeners.push(listener);
  }

  onDuration(listener: (seconds: number) => void): void {
    this.durationListeners.push(listener);
  }

  onError(_listener: () => void): void {
    // A clock never fails to open anything.
  }

  view(): null {
    return null;
  }

  park(): void {
    // No view to take back.
  }

  private tick(): void {
    this.position += 1;
    this.timeListeners.forEach(listener => listener(this.position));
    if (this.position >= this.duration) {
      this.pause();
      this.endListeners.forEach(listener => listener());
    }
  }
}
