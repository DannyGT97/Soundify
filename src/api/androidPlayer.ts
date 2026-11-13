// The player on the phone. Music is played by a native service (media3, see
// `PlaybackService.kt`): it goes on with the screen off and has its own
// notification and lock-screen controls, which the webview's <video> cannot
// give. A VIDEO still needs the webview to draw it, so those files go through
// the HtmlPlayer; the engine in use is whichever loaded the last file.
//
// The native side plays ONE file at a time and the queue stays in `player.ts`.
// The volume is the phone's own: the slider moves the device's music volume and
// the device's buttons move the slider (`volume` events).

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { HtmlPlayer } from './htmlPlayer';
import type { LoadHint, PlayerBackend, Track } from './types';

const VIDEO = new Set(['mp4', 'mkv', 'mov', 'webm', 'avi', 'flv', 'm4v']);

/** Does this file need a picture? Only the webview can draw one. */
export function isVideoFile(path: string | null | undefined): boolean {
  const dot = (path ?? '').lastIndexOf('.');
  return dot >= 0 && VIDEO.has((path ?? '').slice(dot + 1).toLowerCase());
}

/** What the Kotlin player says (`NativePlayer.kt`). */
export type NativeEvent =
  | { kind: 'time'; seconds: number }
  | { kind: 'duration'; seconds: number }
  | { kind: 'ended' }
  | { kind: 'error'; message?: string }
  | { kind: 'playing'; playing: boolean }
  | { kind: 'volume'; volume: number }
  | { kind: 'skip'; direction: 'next' | 'previous' }
  | { kind: 'mode'; mode: 'shuffle' | 'repeat' }
  | { kind: 'open' };

type Engine = 'native' | 'html';

/** The device answers a volume it was just given with an event: that echo is not news (and would fight the slider). */
const ECHO_MS = 400;
/** A reported 0 is asked again after this long before the slider shows it as muted. */
const VOLUME_RECHECK_MS = 300;
/** While the app is in front, the slider is compared with the device this often. */
const VOLUME_POLL_MS = 3000;

export class AndroidPlayer implements PlayerBackend {
  private active: Engine = 'native';
  // The commands go one after another: asynchronous calls could swap their order.
  private chain: Promise<unknown> = Promise.resolve();
  private errorListeners: Array<() => void> = [];
  private volumeSetAt = 0;
  readonly externalVolume = true;

  constructor(private readonly html: HtmlPlayer) {}

  load(blockKey: string, track: Track, hint?: LoadHint): void {
    const wanted: Engine = isVideoFile(track.file) ? 'html' : 'native';
    if (wanted !== this.active) this.stopActive();
    this.active = wanted;
    if (wanted === 'html') {
      this.html.load(blockKey, track);
      return;
    }
    if (!track.file) return;
    const { file, title, id } = track;
    this.send('playerLoad', async () => ({ path: file, title, subtitle: hint?.playlist ?? '', autoplay: hint?.autoplay ?? false, artwork: await this.artwork(id) }));
  }

  play(): void {
    if (this.active === 'html') this.html.play();
    else this.send('playerPlay');
  }

  pause(): void {
    if (this.active === 'html') this.html.pause();
    else this.send('playerPause');
  }

  seek(seconds: number): void {
    if (this.active === 'html') this.html.seek(seconds);
    else this.send('playerSeek', { seconds });
  }

  setVolume(volume: number): void {
    // The phone's music volume covers audio and video alike; the video element stays at full.
    this.volumeSetAt = Date.now();
    this.send('playerSetVolume', { volume });
  }

  unload(): void {
    this.stopActive();
  }

  onTime(listener: (seconds: number) => void): void {
    this.html.onTime(seconds => this.active === 'html' && listener(seconds));
    void this.listen(event => event.kind === 'time' && listener(event.seconds));
  }

  onEnded(listener: () => void): void {
    this.html.onEnded(() => this.active === 'html' && listener());
    void this.listen(event => event.kind === 'ended' && listener());
  }

  onDuration(listener: (seconds: number) => void): void {
    this.html.onDuration(seconds => this.active === 'html' && listener(seconds));
    void this.listen(event => event.kind === 'duration' && listener(event.seconds));
  }

  onError(listener: () => void): void {
    this.errorListeners.push(listener);
    this.html.onError(() => this.active === 'html' && listener());
    void this.listen(event => event.kind === 'error' && listener());
  }

  /** Pause or play done from the notification, the lock screen or headphones. */
  onPlayState(listener: (playing: boolean) => void): void {
    void this.listen(event => event.kind === 'playing' && listener(event.playing));
  }

  /** «Next» / «previous» pressed outside the app. */
  onRemoteSkip(listener: (direction: 'next' | 'previous') => void): void {
    void this.listen(event => event.kind === 'skip' && listener(event.direction));
  }

  /** Shuffle or repeat pressed in the notification. */
  onRemoteMode(listener: (mode: 'shuffle' | 'repeat') => void): void {
    void this.listen(event => event.kind === 'mode' && listener(event.mode));
  }

  /** The app was opened from the music notification: the screen it asked for, now and whenever it is asked again. */
  onRemoteOpen(listener: () => void): void {
    // Kept on the Kotlin side until asked for: at a cold start the page is not listening yet.
    const pull = () =>
      invoke<{ target?: string }>('native_player', { action: 'playerPendingOpen' })
        .then(answer => answer?.target && listener())
        .catch(() => {});
    void this.listen(event => event.kind === 'open' && void pull(), true).then(pull);
    document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && void pull());
  }

  /** The notification's shuffle and repeat buttons draw the page's state. */
  setModes(shuffle: boolean, repeat: 'off' | 'all' | 'one'): void {
    this.send('playerSetModes', { shuffle, repeat });
  }

  /** Where the device's volume is, now and each time the buttons (or the slider) move it. */
  onVolume(listener: (volume: number) => void): void {
    // Asking is the truth; the events are only the quick way. A transient 0 (switching headphones, a
    // call, the system muting a stream for an instant) or an event lost while the webview slept left
    // the slider «muted» while the sound was fine, until the next press: so a 0 is confirmed by asking
    // again, and the slider is re-synchronised on coming back to the app and every few seconds.
    const pull = () =>
      invoke<{ volume?: number }>('native_player', { action: 'playerGetVolume' })
        .then(answer => {
          if (typeof answer?.volume === 'number' && Date.now() - this.volumeSetAt > ECHO_MS) listener(answer.volume);
        })
        .catch(error => console.warn('player: could not read the volume', error));
    void this.listen(event => {
      if (event.kind !== 'volume' || Date.now() - this.volumeSetAt <= ECHO_MS) return;
      if (event.volume > 0) listener(event.volume);
      else window.setTimeout(() => void pull(), VOLUME_RECHECK_MS);
    }, true).then(pull);
    document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && void pull());
    window.addEventListener('focus', () => void pull());
    window.setInterval(() => document.visibilityState === 'visible' && void pull(), VOLUME_POLL_MS);
  }

  view(): HTMLVideoElement | null {
    return this.html.view();
  }

  park(): void {
    this.html.park();
  }

  private stopActive(): void {
    if (this.active === 'html') this.html.unload();
    else this.send('playerStop');
  }

  private listen(handle: (event: NativeEvent) => unknown, always = false): Promise<unknown> {
    return listen<NativeEvent>('native-player', event => {
      // A video's events come from its own element; the native ones are only for audio.
      if (always || this.active === 'native' || event.payload.kind === 'skip' || event.payload.kind === 'mode' || event.payload.kind === 'open') handle(event.payload);
    });
  }

  /**
   * The track's thumbnail for the notification, or '' when it does not arrive in time (the player then
   * shows what the file carries, or the app's logo). The back fetches a missing one in the background, so
   * it is asked again a moment later; it never holds the playback for long.
   */
  private async artwork(videoId: string): Promise<string> {
    for (let attempt = 0; attempt < 3; attempt++) {
      const path = await invoke<string | null>('thumbnail', { id: videoId }).catch(() => null);
      if (path) return path;
      await new Promise(resolve => setTimeout(resolve, 400));
    }
    return '';
  }

  private send(action: string, args?: Record<string, unknown> | (() => Promise<Record<string, unknown>>)): void {
    this.chain = this.chain
      .then(async () => invoke('native_player', { action, args: typeof args === 'function' ? await args() : args }))
      .catch(error => {
        console.warn('player: native command failed', action, error);
        if (action === 'playerLoad') this.errorListeners.forEach(listener => listener());
      });
  }
}
