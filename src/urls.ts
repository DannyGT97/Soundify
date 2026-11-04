// What kind of URL the user typed. Mirrors DownloadPathResolver.isWebUrl and
// the playlist check of the Java app.

export function isWebUrl(text: string): boolean {
  return /^https?:\/\/\S+\.\S+/i.test(text.trim());
}

/**
 * A URL that names a list, as `paths::is_playlist_url` in the back. A video opened
 * from inside a list counts: «Descarga por URL» refuses it, «Crear Playlist» takes it.
 */
export function isPlaylistUrl(text: string): boolean {
  return isWebUrl(text) && /list=|\/playlist/.test(text);
}
