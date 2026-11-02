import { h, titleRow } from '../dom';
import { icon, type IconName } from '../icons';
import { t } from '../i18n';
import { navigate } from '../router';
import { ui, setPlayerPlacement } from '../state';
import { confirmDialog } from '../overlays/confirm';
import { DEFAULT_SCALE, SCALE_STEPS, currentScale, onScaleChange, setScale } from '../scale';
import { themePicker } from '../components/themePicker';

interface Tab {
  id: string;
  icon: IconName;
  build: () => HTMLElement[];
}

const TABS: Tab[] = [
  { id: 'general', icon: 'general', build: general },
  { id: 'interface', icon: 'interface', build: interfaceTab },
  { id: 'updates', icon: 'updates', build: updates },
  { id: 'advanced', icon: 'advanced', build: advanced },
  { id: 'about', icon: 'about', build: about },
];

/** Every setting the search can find: its tab and its i18n key (title + `.desc`). */
const CATALOG: [string, string][] = [
  ['general', 'settings.general.thumbnails'],
  ['general', 'settings.general.includeIds'],
  ['interface', 'settings.interface.theme'],
  ['interface', 'settings.interface.language'],
  ['interface', 'settings.interface.scale'],
  ['interface', 'settings.interface.playerPlacement'],
  ['updates', 'settings.updates.auto'],
  ['advanced', 'settings.advanced.cookies'],
  ['advanced', 'settings.advanced.reset'],
];

let highlight: string | null = null;

/** Ajustes: categories as tabs under the title, and a search across all of them. */
export function settingsView(): HTMLElement {
  const content = h('div', { class: 'stack-20' });
  const tabs = h('div', { class: 'row wrap gap-8' });
  const description = h('p', { class: 'subtitle' });
  const search = h('input', { class: 'input settings-search', attrs: { placeholder: t('settings.search.placeholder') } });

  const showTab = (id: string) => {
    ui.settingsTab = id;
    const tab = TABS.find(x => x.id === id) ?? TABS[0]!;
    tabs.querySelectorAll('button').forEach(b => b.classList.toggle('active', b.dataset.id === tab.id));
    description.textContent = t(`settings.${tab.id}.subtitle`);
    content.replaceChildren(...tab.build());
    if (highlight) {
      const target = content.querySelector<HTMLElement>(`[data-key="${highlight}"]`);
      target?.classList.add('highlight');
      target?.scrollIntoView({ block: 'center' });
      highlight = null;
    }
  };

  for (const tab of TABS) {
    const button = h('button', { class: 'settings-tab', attrs: { 'data-id': tab.id }, onclick: () => {
      search.value = '';
      showTab(tab.id);
    } }, icon(tab.icon, 16), h('span', { text: t(`settings.nav.${tab.id}`) }));
    tabs.append(button);
  }

  search.addEventListener('input', () => {
    const query = search.value.trim().toLowerCase();
    if (!query) {
      showTab(ui.settingsTab);
      return;
    }
    // While there are results, no tab shows as selected.
    tabs.querySelectorAll('button').forEach(b => b.classList.remove('active'));
    const hits = CATALOG.filter(([, key]) => `${t(key)} ${t(`${key}.desc`)}`.toLowerCase().includes(query));
    content.replaceChildren(...(hits.length ? hits.map(([tab, key]) => h('button', { class: 'card search-hit', onclick: () => {
      search.value = '';
      highlight = key;
      showTab(tab);
    } },
      h('div', { class: 'section-title', text: t(key) }),
      h('div', { class: 'subtitle', text: `${t('settings.search.in')} ${t(`settings.nav.${tab}`)} · ${t(`${key}.desc`)}` })))
      : [h('p', { class: 'help', text: t('settings.search.empty') })]));
  });

  showTab(ui.settingsTab);
  return h('div', { class: 'screen' },
    h('div', { class: 'screen-head' },
      titleRow('settings', t('settings.title'), h('div', { class: 'grow' }), search),
      description),
    tabs,
    content);
}

// === PANELS ===

function setting(key: string, control: HTMLElement | null, below?: HTMLElement): HTMLElement {
  return h('section', { class: 'card settings-card', attrs: { 'data-key': key } },
    h('div', { class: 'row center gap-14' },
      h('div', { class: 'grow' },
        h('div', { class: 'section-title', text: t(key) }),
        h('p', { class: 'subtitle', text: t(`${key}.desc`) })),
      control),
    below);
}

function checkbox(checked: boolean): HTMLInputElement {
  const box = h('input', { class: 'switch', attrs: { type: 'checkbox' } });
  box.checked = checked;
  return box;
}

function select(options: [string, string][], value: string, changed?: (v: string) => void): HTMLSelectElement {
  const el = h('select', { class: 'input select-fit' }, ...options.map(([v, label]) => h('option', { text: label, attrs: { value: v } })));
  el.value = value;
  if (changed) el.addEventListener('change', () => changed(el.value));
  return el;
}

function general(): HTMLElement[] {
  return [
    setting('settings.general.thumbnails', checkbox(true)),
    setting('settings.general.includeIds', checkbox(false)),
  ];
}

function interfaceTab(): HTMLElement[] {
  return [
    setting('settings.interface.theme', null, themePicker()),
    setting('settings.interface.playerPlacement', select([
      ['side', t('player.placement.side')], ['bottom', t('player.placement.bottom')], ['both', t('player.placement.both')],
    ], ui.playerPlacement, value => setPlayerPlacement(value as 'side' | 'bottom' | 'both'))),
    setting('settings.interface.language', select([['es', 'Español'], ['en', 'English'], ['ca', 'Català']], 'es')),
    setting('settings.interface.scale', scaleSelect()),
  ];
}

/** The scale list; it follows Ctrl + / Ctrl − too, while the tab is on screen. */
function scaleSelect(): HTMLSelectElement {
  const el = select(SCALE_STEPS.map(p => [String(p), `${p}%`]), String(currentScale()), v => setScale(Number(v)));
  const stop = onScaleChange(percent => {
    if (!el.isConnected) {
      stop();
      return;
    }
    el.value = String(percent);
  });
  return el;
}

function updates(): HTMLElement[] {
  return [
    h('section', { class: 'card settings-card' },
      h('div', { class: 'section-title', text: t('settings.updates.currentVersion') }),
      h('p', { class: 'subtitle', text: t('app.version') })),
    h('section', { class: 'card settings-card' },
      h('div', { class: 'section-title', text: t('settings.updates.title') }),
      h('p', { class: 'subtitle', text: t('settings.updates.check.desc') }),
      h('div', {}, h('button', { class: 'primary-button', text: t('settings.updates.checkNow') }))),
    setting('settings.updates.auto', checkbox(true)),
  ];
}

function advanced(): HTMLElement[] {
  const path = h('input', { class: 'input grow', attrs: { readonly: '', placeholder: t('settings.advanced.cookies.placeholder') } });
  return [
    setting('settings.advanced.cookies', null, h('div', { class: 'row center gap-10' }, path,
      h('button', { class: 'secondary-button', text: t('common.select'), onclick: () => { path.value = 'C:\\Users\\cookies.txt'; } }),
      h('button', { class: 'secondary-button', text: t('common.remove'), onclick: () => { path.value = ''; } }))),
    setting('settings.advanced.reset', h('button', { class: 'danger-button', text: t('settings.advanced.reset.button'), onclick: () =>
      confirmDialog(t('settings.advanced.reset'), t('settings.advanced.reset'), t('settings.advanced.reset.desc'), [
        // Like the Java app: a reset is applied on the spot, not only written down.
        { label: t('settings.advanced.reset.button'), kind: 'danger', run: () => { setScale(DEFAULT_SCALE); navigate('settings'); } },
      ]) })),
  ];
}

function about(): HTMLElement[] {
  return [
    h('section', { class: 'card settings-card' },
      h('div', { class: 'row center gap-14' },
        h('img', { attrs: { src: '/icons/soundify-logo.png', alt: '', width: '40', height: '40' } }),
        h('div', { class: 'grow' },
          h('div', { class: 'section-title', text: 'Soundify' }),
          h('p', { class: 'subtitle', text: t('app.description') })),
        h('span', { class: 'chip', text: t('app.version') }))),
    h('section', { class: 'card settings-card' },
      h('div', { class: 'row center gap-14' },
        h('div', { class: 'grow' },
          h('div', { class: 'section-title', text: t('settings.about.license') }),
          h('p', { class: 'subtitle', text: t('settings.about.license.desc') })),
        h('span', { class: 'chip', text: 'GPL-3.0-or-later' }))),
    h('section', { class: 'card settings-card' },
      h('div', { class: 'section-title', text: t('settings.about.tools') }),
      h('p', { class: 'subtitle', text: 'yt-dlp — Unlicense' }),
      h('p', { class: 'subtitle', text: 'FFmpeg — GPL v3' }),
      h('p', { class: 'subtitle', text: 'Deno — MIT' })),
    h('section', { class: 'card settings-card' }, h('p', { class: 'subtitle', text: t('settings.about.author') })),
  ];
}
