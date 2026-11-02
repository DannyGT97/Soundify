// A tiny element builder: views are built from nodes, never from HTML strings,
// so titles coming from YouTube can never be read as markup.

import { icon, type IconName } from './icons';
import { homeLogo } from './components/logo';

type Child =Node | string | number | null | undefined | false;

interface Props {
  class?: string;
  text?: string;
  title?: string;
  onclick?: (e: MouseEvent) => void;
  attrs?: Record<string, string>;
}

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Props = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (props.class) el.className = props.class;
  if (props.text !== undefined) el.textContent = props.text;
  if (props.title) {
    el.title = props.title;
    el.setAttribute('aria-label', props.title);
  }
  const onclick = props.onclick;
  if (onclick) el.addEventListener('click', e => onclick(e as MouseEvent));
  for (const [name, value] of Object.entries(props.attrs ?? {})) el.setAttribute(name, value);
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : String(child));
  }
  return el;
}

/** The title row every screen opens with, at the same height as the menu's header. */
export function titleRow(iconName: IconName | null, title: string, ...extra: Child[]): HTMLElement {
  return h('div', { class: 'title-row' },
    iconName ? h('span', { class: 'title-icon' }, iconName === 'home' ? homeLogo(26) : icon(iconName, 26)) : null,
    h('h1', { class: 'page-title', text: title }),
    ...extra);
}

export function card(...children: Child[]): HTMLElement {
  return h('section', { class: 'card' }, ...children);
}

/** The numbered heading of a form card ("01 URL"). */
export function stepHeading(step: string, title: string): HTMLElement {
  return h('div', { class: 'step-heading' },
    h('span', { class: 'step-number', text: step }),
    h('span', { class: 'card-title', text: title }));
}
