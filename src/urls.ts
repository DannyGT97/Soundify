// What kind of URL the user typed. Mirrors DownloadPathResolver.isWebUrl and
// the playlist check of the Java app.

export function isWebUrl(text: string): boolean {
  return /^https?:\/\/\S+\.\S+/i.test(text.trim());
}

/** A playlist and nothing else: with `v=` too, it is a video opened from inside a playlist. */
export function isPlaylistUrl(text: string): boolean {
  return isWebUrl(text) && /[?&]list=/.test(text) && !/[?&]v=/.test(text);
}
