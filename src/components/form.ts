// Pieces shared by "Descarga por URL" and "Crear Playlist": the cover beside the
// URL, the format button with its panel and the two destinations.

import { h } from '../dom';
import { icon } from '../icons';
import { t } from '../i18n';
import { api, type DeviceTarget } from '../api';
import { showPicture, type Picture } from '../covers';
import { cover } from './cover';
import { library } from '../library';
import { isPlayable } from '../player';
import { canDownload, tools } from '../tools';
import { navigate } from '../router';
import { deviceLabel, openDevicePicker } from '../overlays/devicePicker';
import { closeFloating, openFloating } from './floating';

// === FORMAT ===

export interface FormatPicker {
  node: HTMLElement;
  value: () => string;
}

function formatNote(format: string): HTMLElement {
  return isPlayable(format)
    ? h('small', { text: t(`format.note.${format}`) })
    : h('small', { class: 'warn', text: t('format.note.notPlayable') });
}

/** A button that says what is chosen and opens the whole list, audio beside video. */
export function formatPicker(changed: () => void): FormatPicker {
  const { audio, video } = library.formats;
  let chosen = audio[0] ?? video[0] ?? 'MP3';

  const button = h('button', { class: 'fmt-btn', attrs: { 'aria-haspopup': 'menu' } });
  const paint = () => {
    button.replaceChildren(
      h('span', { class: 'fmt', text: chosen }),
      h('div', { class: 'min0' }, h('b', { text: t(video.includes(chosen) ? 'format.video' : 'format.audio') }), formatNote(chosen)),
      icon('chevron-down', 18));
  };
  const row = (format: string) => h('button', {
    class: `fmt-row${format === chosen ? ' on' : ''}`,
    attrs: { role: 'menuitemradio', 'aria-checked': String(format === chosen) },
    onclick: () => {
      chosen = format;
      closeFloating();
      paint();
      changed();
    },
  }, h('b', { text: format }), formatNote(format), format === chosen ? icon('ok', 15) : null);
  const column = (titleKey: string, formats: string[]) =>
    h('div', {}, h('h4', { text: t(titleKey) }), ...formats.map(row));

  button.addEventListener('click', () => {
    openFloating(button, [h('div', { class: 'fmt-panel' }, column('format.audio', audio), column('format.video', video))], { alignLeft: true });
  });
  paint();
  return { node: button, value: () => chosen };
}

/** Whether a picture picked by hand can end up in a loose file (the same list as `cover_embed::can_embed`). */
export function keepsPickedCover(format: string): boolean {
  return !['WAV', 'MKV', 'WEBM', 'AVI', 'FLV'].includes(format.toUpperCase());
}

// === THE PHONE'S SWITCH ===

/** The phone has one tab for Descarga por URL and Crear Playlist: this goes from one to the other. */
export function formSwitch(current: 'download' | 'new-playlist'): HTMLElement {
  const option = (route: 'download' | 'new-playlist', key: string) =>
    h('button', { class: route === current ? 'on' : '', text: t(key), onclick: () => navigate(route) });
  return h('div', { class: 'form-switch' }, option('download', 'nav.download'), option('new-playlist', 'nav.newPlaylist'));
}

// === DESTINATION ===

export interface Destinations {
  node: HTMLElement;
  /** The folder, or how the chosen phone folder reads ("Pixel 8 / Music"); empty = nothing chosen. */
  value: () => string;
  /** The phone folder when that is what was chosen; null for a folder of this computer. */
  device: () => DeviceTarget | null;
}

/** "Carpeta local" and "Dispositivo MTP", each saying what was chosen once it is. */
export function destinations(changed: () => void): Destinations {
  let text = '';
  let device: DeviceTarget | null = null;
  const set = (value: string, target: DeviceTarget | null) => {
    text = value;
    device = target;
    paint();
    changed();
  };
  const card = (iconName: 'folder' | 'device', titleKey: string, emptyKey: string, pick: () => void) => {
    const small = h('small', { class: 'ellipsis' });
    const node = h('button', { class: 'dest', onclick: pick }, icon(iconName, 20),
      h('div', { class: 'min0' }, h('b', { text: t(titleKey) }), small));
    return { node, paint: (on: boolean) => {
      node.classList.toggle('on', on);
      small.textContent = on ? text : t(emptyKey);
      small.title = on ? text : '';
    } };
  };
  const folder = card('folder', 'form.dest.local', 'form.dest.local.empty', () => {
    void api.pickFolder(t('download.folder.choose')).then(chosen => { if (chosen) set(chosen, null); });
  });
  const phone = card('device', 'form.dest.device', 'form.dest.device.empty', () => {
    openDevicePicker('destination', target => set(deviceLabel(target), target));
  });
  // Nothing to plug a phone into from a phone.
  phone.node.classList.add('desktop-only');
  function paint(): void {
    folder.paint(!!text && !device);
    phone.paint(!!text && !!device);
  }
  paint();
  return { node: h('div', { class: 'dests' }, folder.node, phone.node), value: () => text, device: () => device };
}

// === COVER ===

export interface UrlCover {
  node: HTMLElement;
  /** Looks for the picture of this URL, once typing stops; null = there is none to look for. */
  look: (url: string | null) => void;
  /** The file picked by hand. */
  picked: () => string | null;
  /** The file found for the URL, when it is a file (a YouTube video's goes by its id). */
  found: () => string | null;
  loading: () => boolean;
}

/**
 * The square beside the URL: empty, looking, the URL's own picture or the one
 * picked by hand. The picked one survives a change of URL; its × goes back.
 * `playlist` = the URL names a playlist, so its own picture comes first and
 * its first video is the fallback; for a loose video, the other way round.
 */
export function urlCover(playlist: boolean, emptyKey: string, changed: () => void): UrlCover {
  let picked: string | null = null;
  let auto: Picture | null = null;
  let looking = false;
  let asked: string | null = null;
  let timer: number | undefined;

  const art = cover('', false, 104);
  const tile = h('button', { class: 'url-cover', onclick: async () => {
    const image = await api.pickCoverImage(t('newPlaylist.cover.title'), t('cover.choose.filter'));
    if (!image) return;
    picked = image;
    paint();
    changed();
  } });
  const clear = h('button', { class: 'cover-x', onclick: () => {
    picked = null;
    paint();
    changed();
  } }, icon('close', 14));

  function paint(): void {
    const picture: Picture | null = picked ? { kind: 'file', path: picked } : auto;
    clear.hidden = !picked;
    const clearTitle = t(auto ? 'form.cover.backToAuto' : 'form.cover.remove');
    clear.title = clearTitle;
    clear.setAttribute('aria-label', clearTitle);
    tile.classList.toggle('is-empty', !picture && !looking);
    tile.classList.toggle('loading', !picture && looking);
    if (picture) {
      tile.title = t('form.cover.change');
      showPicture(art, picture);
      tile.replaceChildren(art,
        h('span', { class: 'source', text: t(picked ? 'form.cover.mine' : 'form.cover.auto') }),
        h('span', { class: 'shade' }, icon('image-plus', 18), t('form.cover.changeShort')));
    } else if (looking) {
      tile.title = t('form.cover.looking');
      tile.replaceChildren(icon('image', 24));
    } else {
      tile.title = t('form.cover.emptyHint');
      tile.replaceChildren(icon('image-plus', 24), t(emptyKey));
    }
  }

  const fromFile = (url: string) => api.urlThumbnail(url, playlist)
    .catch(() => null)
    .then((path): Picture | null => path ? { kind: 'file', path } : null);
  const fromVideo = (url: string) => api.firstVideoId(url)
    .catch(() => null)
    .then((id): Picture | null => id ? { kind: 'thumbnail', id } : null);
  const search = async (url: string) => playlist
    ? await fromFile(url) ?? await fromVideo(url)
    : await fromVideo(url) ?? await fromFile(url);

  paint();
  return {
    node: h('div', { class: 'cover-box' }, tile, clear),
    look: url => {
      if (url === asked) return;
      asked = url;
      window.clearTimeout(timer);
      auto = null;
      // Asking needs yt-dlp; without it the square just waits for a picked picture.
      looking = !!url && canDownload(tools.report);
      paint();
      changed();
      if (!looking || !url) return;
      // Asked once typing stops (it may go to YouTube); an answer for a URL
      // that is no longer in the field is dropped.
      timer = window.setTimeout(() => {
        void search(url).then(picture => {
          if (asked !== url) return;
          auto = picture;
          looking = false;
          paint();
          changed();
        });
      }, 700);
    },
    picked: () => picked,
    found: () => (auto?.kind === 'file' ? auto.path : null),
    loading: () => looking,
  };
}

// === THE SCREEN ===

/** The label over a field. */
export function fieldLabel(key: string): HTMLElement {
  return h('span', { class: 'field-label', text: t(key) });
}

/** What goes in the footer: the format and the destination, and what is missing. */
export function footerSummary(): { node: HTMLElement; show: (head: string, line: string) => void } {
  const head = h('b', { class: 'ellipsis' });
  const line = h('small');
  return {
    node: h('div', { class: 'fp-sum' }, head, line),
    show: (headText, lineText) => {
      head.textContent = headText;
      head.title = headText;
      line.textContent = lineText;
    },
  };
}
