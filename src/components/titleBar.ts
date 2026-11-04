// The window's own title bar (the system's is off: `decorations: false`), in the
// colours of the theme. Dragging, double click to maximize and dragging to the
// screen's edges come from `data-tauri-drag-region`; resizing from the borders,
// from the window itself. The buttons block the drag by being buttons.

import type { WindowControls } from '../api';
import { h } from '../dom';
import { icon, type IconName } from '../icons';
import { onLanguageChange } from '../i18n';
import { describe } from './transport';

export function buildTitleBar(controls: WindowControls): HTMLElement {
  const button = (name: IconName, action: () => void, extra = '') =>
    h('button', { class: `titlebar-button ${extra}`, onclick: action }, icon(name, 16));
  const minimize = button('window-minimize', controls.minimize);
  const maximize = button('window-maximize', controls.toggleMaximize);
  const close = button('close', controls.close, 'titlebar-close');
  const bar = h('header', { class: 'titlebar', attrs: { 'data-tauri-drag-region': 'deep' } },
    h('span', { class: 'titlebar-name', text: 'Soundify' }), minimize, maximize, close);
  // The menu of the system's title bar (restore, move, size…), which has no bar to open from now.
  bar.addEventListener('contextmenu', event => {
    event.preventDefault();
    controls.showSystemMenu();
  });

  let maximized = false;
  const labels = () => {
    describe(minimize, 'window.minimize');
    describe(maximize, maximized ? 'window.restore' : 'window.maximize');
    describe(close, 'window.close');
  };
  // By the window, not by the button: the double click, the screen's edges and Win+↑ also change it.
  const readState = () => void controls.isMaximized().then(now => {
    if (now === maximized) return;
    maximized = now;
    maximize.replaceChildren(icon(now ? 'window-restore' : 'window-maximize', 16));
    labels();
  });
  controls.onResized(readState);
  onLanguageChange(labels);
  labels();
  readState();
  return bar;
}
