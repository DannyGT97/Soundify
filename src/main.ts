import './styles/theme.css';
import './styles/app.css';
import { h } from './dom';
import { startRouter } from './router';
import { initScale } from './scale';
import { buildPlayerBar } from './components/playerBar';
import { buildSidebar, markActive } from './components/sidebar';
import { downloadView } from './views/download';
import { homeView } from './views/home';
import { managerView } from './views/manager';
import { newPlaylistView } from './views/newPlaylist';
import { nowPlayingView } from './views/nowPlaying';
import { playerView } from './views/player';
import { settingsView } from './views/settings';
import { mountToasts } from './components/toasts';
import { ui } from './state';

// The shell: menu and page side by side, the player bar under both, and the
// layer where the windows that open on top live.
const app = document.getElementById('app');
if (app) {
  initScale();
  document.documentElement.dataset.theme = ui.theme;

  app.append(
    h('div', { class: 'workspace' }, buildSidebar(), h('main', { attrs: { id: 'page' } })),
    buildPlayerBar(),
    h('div', { attrs: { id: 'overlay-root' } }));
  mountToasts(app);

  startRouter({
    'home': homeView,
    'download': downloadView,
    'new-playlist': newPlaylistView,
    'manager': managerView,
    'player': playerView,
    'now-playing': nowPlayingView,
    'settings': settingsView,
  }, markActive);
}
