// Picture in picture, inside the app only: a small panel floating over every
// screen, holding the engine's own video element while the user moves around.
// Not `requestPictureInPicture` nor a second window: minimised, it goes with the
// app. Closing it pauses, and the file stays loaded at the same second.

import { h } from '../dom';
import { icon } from '../icons';
import { onLanguageChange, t } from '../i18n';
import * as player from '../player';
import { clampPanel, overDock, pipSize, startSpot, type Point } from '../pipPanel';
import { isPhone } from '../platform';
import { navigate } from '../router';
import { subscribe, subscribeWhile, ui } from '../state';
import { control, describe } from './transport';

/** The way in, shown only while a video is what is loaded. Pressed again, it takes the panel away. */
export function pipButton(): HTMLButtonElement {
  const button = control('pip', 'pip.open', () => ui.pip ? player.hidePip() : player.showPip());
  button.classList.add('pip-button');
  // Inside Reproduciendo a double click on the picture means full screen.
  button.addEventListener('dblclick', event => event.stopPropagation());
  const refresh = () => {
    button.hidden = !player.currentIsVideo();
    button.classList.toggle('on', ui.pip);
    describe(button, ui.pip ? 'pip.hide' : 'pip.open');
  };
  subscribeWhile(button, 'player', refresh);
  refresh();
  return button;
}

/** The panel itself: one for the whole app, mounted in the shell so it survives the navigation. */
export function mountPip(parent: HTMLElement): void {
  const stage = h('div', { class: 'pip-stage' });
  const title = h('span', { class: 'pip-title ellipsis' });
  const toggle = h('button', { class: 'player-control', onclick: player.togglePlay });
  const expandPanel = () => {
    player.hidePip();
    navigate('now-playing');
  };
  const expand = h('button', { class: 'player-control', onclick: expandPanel }, icon('pip-expand', 18));
  const close = h('button', { class: 'player-control', onclick: player.closePip }, icon('close', 18));
  const panel = h('div', { class: 'pip-panel', attrs: { role: 'region' } },
    stage, h('div', { class: 'pip-bar' }, title, toggle, expand, close));
  panel.hidden = true;
  parent.append(panel);

  const room = () => ({ width: window.innerWidth, height: window.innerHeight });
  // Smaller on a phone; measured each time, since the window can change shape.
  const size = () => {
    const s = pipSize(isPhone());
    panel.style.width = `${s.width}px`;
    panel.style.height = `${s.height}px`;
    return s;
  };
  let spot: Point | null = null;
  const place = (wanted: Point) => {
    const titleBar = document.querySelector<HTMLElement>('.titlebar')?.offsetHeight ?? 0;
    spot = clampPanel(wanted, size(), room(), titleBar);
    panel.style.left = `${spot.x}px`;
    panel.style.top = `${spot.y}px`;
  };

  // === DRAGGING === (from anywhere but its buttons)
  // Reproduciendo keeps a slot for the video while the panel has it: held over
  // it, the slot lights up, and letting go there puts the video back.
  let grabbed: Point | null = null;
  let dock: HTMLElement | null = null;
  const dockSlot = () => document.querySelector<HTMLElement>('.np-pip-here');
  const hoverDock = () => {
    const slot = dockSlot();
    const over = !!slot && overDock(panel.getBoundingClientRect(), slot.getBoundingClientRect());
    const target = over ? slot : null;
    if (target === dock) return;
    dock?.classList.remove('dock-over');
    target?.classList.add('dock-over');
    dock = target;
  };
  panel.addEventListener('pointerdown', event => {
    if (!spot || (event.target as Element).closest('button')) return;
    grabbed = { x: event.clientX - spot.x, y: event.clientY - spot.y };
    panel.setPointerCapture(event.pointerId);
    panel.classList.add('dragging');
  });
  panel.addEventListener('pointermove', event => {
    if (!grabbed) return;
    place({ x: event.clientX - grabbed.x, y: event.clientY - grabbed.y });
    hoverDock();
  });
  const letGo = (event: Event) => {
    const dropped = event.type === 'pointerup' && grabbed !== null && dock !== null;
    grabbed = null;
    panel.classList.remove('dragging');
    dock?.classList.remove('dock-over');
    dock = null;
    if (dropped) player.hidePip();
  };
  panel.addEventListener('pointerup', letGo);
  panel.addEventListener('pointercancel', letGo);
  panel.addEventListener('dblclick', event => {
    if (!(event.target as Element).closest('button')) expandPanel();
  });
  window.addEventListener('resize', () => { if (spot) place(spot); });

  const labels = () => {
    panel.setAttribute('aria-label', t('pip.panel'));
    describe(expand, 'pip.expand');
    describe(close, 'pip.close');
    describe(toggle, ui.playing ? 'player.pause' : 'player.play');
  };
  let holding = false;
  const refresh = () => {
    panel.hidden = !ui.pip;
    if (!ui.pip) {
      // The element is already out (`hidePip` parks it first): this only drops the mock's placeholder.
      if (holding) stage.replaceChildren();
      holding = false;
      return;
    }
    if (!holding) {
      holding = true;
      // The engine's own element, moved in; where it draws nothing (the mock), a placeholder.
      stage.replaceChildren(player.videoView() ?? h('div', { class: 'np-video' }, icon('play', 36, true)));
      place(spot ?? startSpot(size(), room(), isPhone()));
    }
    title.textContent = ui.nowPlaying?.track.title ?? '';
    toggle.replaceChildren(icon(ui.playing ? 'pause' : 'play', 18, true));
    labels();
  };
  // Not `subscribeWhile`: the panel is never drawn again, and it starts hidden.
  subscribe('player', refresh);
  onLanguageChange(labels);
  labels();
  refresh();
}
