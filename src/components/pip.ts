// Picture in picture, inside the app only: a small panel floating over every
// screen, holding the engine's own video element while the user moves around.
// Not `requestPictureInPicture` nor a second window: minimised, it goes with the
// app. Closing it pauses, and the file stays loaded at the same second.

import { h } from '../dom';
import { icon } from '../icons';
import { onLanguageChange, t } from '../i18n';
import * as player from '../player';
import { clampPanel, PIP_SIZE, startSpot, type Point } from '../pipPanel';
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
  panel.style.width = `${PIP_SIZE.width}px`;
  panel.style.height = `${PIP_SIZE.height}px`;
  panel.hidden = true;
  parent.append(panel);

  const room = () => ({ width: window.innerWidth, height: window.innerHeight });
  let spot: Point | null = null;
  const place = (wanted: Point) => {
    const titleBar = document.querySelector<HTMLElement>('.titlebar')?.offsetHeight ?? 0;
    spot = clampPanel(wanted, PIP_SIZE, room(), titleBar);
    panel.style.left = `${spot.x}px`;
    panel.style.top = `${spot.y}px`;
  };

  // === DRAGGING === (from anywhere but its buttons)
  let grabbed: Point | null = null;
  panel.addEventListener('pointerdown', event => {
    if (!spot || (event.target as Element).closest('button')) return;
    grabbed = { x: event.clientX - spot.x, y: event.clientY - spot.y };
    panel.setPointerCapture(event.pointerId);
    panel.classList.add('dragging');
  });
  panel.addEventListener('pointermove', event => {
    if (grabbed) place({ x: event.clientX - grabbed.x, y: event.clientY - grabbed.y });
  });
  const letGo = () => {
    grabbed = null;
    panel.classList.remove('dragging');
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
      place(spot ?? startSpot(PIP_SIZE, room()));
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
