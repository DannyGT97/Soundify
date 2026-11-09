import './styles/theme.css';
import './styles/app.css';
import './styles/screens.css';
import './styles/phone.css';
import { h } from './dom';
import { currentRoute, navigate, startRouter } from './router';
import { loadPreferences } from './preferences';
import { buildPlayerBar } from './components/playerBar';
import { mountPip } from './components/pip';
import { buildTitleBar } from './components/titleBar';
import { buildSidebar, markActive } from './components/sidebar';
import { downloadView } from './views/download';
import { homeView } from './views/home';
import { managerView } from './views/manager';
import { newPlaylistView } from './views/newPlaylist';
import { nowPlayingView } from './views/nowPlaying';
import { playerView } from './views/player';
import { settingsView } from './views/settings';
import { mountToasts, notify } from './components/toasts';
import { api } from './api';
import { onLanguageChange, t } from './i18n';
import { loadLibrary, loadLibrarySoon } from './library';
import { restoreLast, savePosition } from './player';
import { loadAppInfo } from './appInfo';
import { watchAppUpdates } from './appUpdate';
import { messageText, noticeTone, pointsToSettings } from './tasks';
import { loadTools, missingRequired, openAdvancedSettings, tools, watchTools } from './tools';
import { openToolsSetup } from './overlays/toolsSetup';
import { installSpaceToPlay } from './keys';
import { subscribe, ui } from './state';
import { installPlatform, onAndroid } from './platform';

// The shell: menu and page side by side, the player bar under both, and the
// layer where the windows that open on top live.
const app = document.getElementById('app');
if (app) {
  // Before the first screen: every view reads the library synchronously, and
  // nothing is drawn in the wrong look or language.
  installPlatform();
  await Promise.all([loadLibrary(), loadAppInfo(), loadPreferences()]);
  // The last file, paused at its second (it needs the library to find it).
  await restoreLast();
  api.onClosing(savePosition);

  let sidebar = buildSidebar();
  let playerBar = buildPlayerBar();
  if (api.windowControls) app.append(buildTitleBar(api.windowControls));
  app.append(
    h('div', { class: 'workspace' }, sidebar, h('main', { attrs: { id: 'page' } })),
    playerBar,
    h('div', { attrs: { id: 'overlay-root' } }));
  // Another language: the menu and the bar are drawn again in place, and the
  // screen too (keeping where it was); `page` and the overlays stay.
  onLanguageChange(() => {
    const freshSidebar = buildSidebar();
    const freshBar = buildPlayerBar();
    sidebar.replaceWith(freshSidebar);
    playerBar.replaceWith(freshBar);
    sidebar = freshSidebar;
    playerBar = freshBar;
    const { route, params } = currentRoute();
    navigate(route, params);
  });
  mountPip(app);
  mountToasts(app);
  installSpaceToPlay();
  watchToolHealth();
  watchTools();
  watchAppUpdates();
  api.watchDownloads(notice => {
    const text = messageText(notice.key, notice.args);
    // YouTube's bot check: it stays until closed and takes the user to the cookies setting.
    if (pointsToSettings(notice)) {
      notify({ tone: noticeTone(notice), text, persistent: true, action: { label: t('activity.botcheck.openSettings'), run: openAdvancedSettings } });
    } else notify({ tone: noticeTone(notice), text });
  });
  // A download changes the library (new files, a new playlist): the Gestor and
  // the Reproductor read it again once the task ends.
  let wasRunning = ui.task !== null;
  subscribe('tasks', () => {
    if (wasRunning && ui.task === null) void loadLibrary();
    wasRunning = ui.task !== null;
  });
  // And while it runs, file by file: «Ver archivos» and the Gestor show each
  // one as soon as it is on disk, not all of them at the end.
  api.onFileDownloaded(() => loadLibrarySoon());

  startRouter({
    'home': homeView,
    'download': downloadView,
    'new-playlist': newPlaylistView,
    'manager': managerView,
    'player': playerView,
    'now-playing': nowPlayingView,
    'settings': settingsView,
  }, route => {
    app.dataset.route = route;
    markActive(route);
  });
  // The first start (or tools deleted by hand): the assistant, only when a
  // REQUIRED one is missing. Asked after the first screen, so it is not waited for.
  // Android has nothing to install them with yet: Inicio and Descarga say so.
  void loadTools().then(() => {
    if (!onAndroid && missingRequired(tools.report).length) openToolsSetup();
  }).catch(() => {});
}

/**
 * One notice, kept until closed, when the download tools are not all in place:
 * on the Java app a missing JS runtime only reached a log nobody reads, and a
 * playlist's 197 files failed without the user knowing why. A worse
 * consequence replaces the notice instead of stacking a second one.
 */
function watchToolHealth(): void {
  let dismiss: (() => void) | null = null;
  api.watchToolHealth(consequenceKey => {
    dismiss?.();
    dismiss = notify({ tone: 'warning', persistent: true, text: t('toolHealth.message', t(consequenceKey)) });
  });
}
