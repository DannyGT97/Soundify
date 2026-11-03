// The real engine on desktop: the webview's own media element, playing the
// file straight from the music folder through the asset protocol (the back
// lets the webview into each folder that holds a file, see `list_playlists`).
//
// It is ONE <video> for audio and video alike, so a file switching from one to
// the other needs nothing special. It lives in a hidden holder in <body> and
// never leaves the document: a media element taken out of the page pauses on
// its own, so Now Playing MOVES it into its box and back (a move inside the
// same task does not count as leaving).

import type { PlayerBackend, Track } from './types';

export class HtmlPlayer implements PlayerBackend {
  private readonly element: HTMLVideoElement;
  private readonly holder: HTMLElement;
  private loaded = false;

  constructor(private readonly urlOf: (path: string) => string) {
    this.element = document.createElement('video');
    this.element.preload = 'auto';
    this.element.playsInline = true;
    this.element.className = 'np-media';
    this.holder = document.createElement('div');
    this.holder.hidden = true;
    this.holder.append(this.element);
    document.body.append(this.holder);
  }

  load(_blockKey: string, track: Track): void {
    this.element.pause();
    this.loaded = !!track.file;
    if (track.file) this.element.src = this.urlOf(track.file);
    else this.element.removeAttribute('src');
    this.element.load();
  }

  play(): void {
    // A rejected play() (the file failed, or a pause came first) is reported
    // by the element's own `error` event, or is no error at all.
    this.element.play().catch(() => {});
  }

  pause(): void {
    this.element.pause();
  }

  seek(seconds: number): void {
    if (this.loaded) this.element.currentTime = seconds;
  }

  setVolume(volume: number): void {
    this.element.volume = Math.max(0, Math.min(1, volume));
  }

  unload(): void {
    this.loaded = false;
    this.element.pause();
    this.element.removeAttribute('src');
    this.element.load();
    this.holder.append(this.element);
  }

  onTime(listener: (seconds: number) => void): void {
    this.element.addEventListener('timeupdate', () => listener(this.element.currentTime));
  }

  onEnded(listener: () => void): void {
    this.element.addEventListener('ended', () => listener());
  }

  onDuration(listener: (seconds: number) => void): void {
    this.element.addEventListener('loadedmetadata', () => {
      const seconds = this.element.duration;
      listener(Number.isFinite(seconds) ? seconds : 0);
    });
  }

  onError(listener: () => void): void {
    // Emptying the element on unload is not a failure.
    this.element.addEventListener('error', () => { if (this.loaded) listener(); });
  }

  view(): HTMLVideoElement {
    return this.element;
  }

  park(): void {
    this.holder.append(this.element);
  }
}
