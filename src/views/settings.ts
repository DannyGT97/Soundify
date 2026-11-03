import { h, titleRow } from '../dom';
import { icon, type IconName } from '../icons';
import { appVersion } from '../appInfo';
import { DICTIONARIES, LANGUAGES, language, t } from '../i18n';
import { api } from '../api';
import { navigate } from '../router';
import { subscribeWhile, ui, type PlayerPlacement } from '../state';
import { confirmDialog } from '../overlays/confirm';
import { SCALE_STEPS, currentScale, onScaleChange, setScale } from '../scale';
import { themePicker } from '../components/themePicker';
import { notify } from '../components/toasts';
import { chooseLanguage, choosePlayerPlacement, preferences, resetPreferences, save } from '../preferences';
import { buildIndex, searchIndex } from '../settingsSearch';
import { checkForUpdate, onUpdateChange, updateState } from '../appUpdate';
import { checkYtdlp, installTools, isBusy, loadTools, missingTools, toolChip, tools, TOOL_SIZES_MB } from '../tools';
import type { ToolStatus } from '../api/types';

interface Tab {
  id: string;
  icon: IconName;
  build: () => HTMLElement[];
}

const TABS: Tab[] = [
  { id: 'general', icon: 'general', build: general },
  { id: 'interface', icon: 'interface', build: interfaceTab },
  { id: 'updates', icon: 'updates', build: updates },
  { id: 'tools', icon: 'tools', build: toolsTab },
  { id: 'advanced', icon: 'advanced', build: advanced },
  { id: 'about', icon: 'about', build: about },
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
    if (tab.id === 'tools') button.append(missingBadge());
    tabs.append(button);
  }

  // Built from the keys of the language in use (see settingsSearch.ts).
  const index = buildIndex(DICTIONARIES[language()]);
  search.addEventListener('input', () => {
    if (!search.value.trim()) {
      showTab(ui.settingsTab);
      return;
    }
    // While there are results, no tab shows as selected.
    tabs.querySelectorAll('button').forEach(b => b.classList.remove('active'));
    const hits = searchIndex(index, search.value);
    content.replaceChildren(...(hits.length ? hits.map(hit => h('button', { class: 'card search-hit', onclick: () => {
      search.value = '';
      highlight = hit.key;
      showTab(hit.category);
    } },
      h('div', { class: 'section-title', text: hit.title }),
      h('div', { class: 'subtitle', text: `${t('settings.search.in')} ${t(`settings.nav.${hit.category}`)} · ${hit.description}` })))
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

/** A switch saved as soon as it is flipped. */
function checkbox(field: 'downloadThumbnails' | 'includeIdInTitleName' | 'appUpdateCheckEnabled' | 'autoUpdates'): HTMLInputElement {
  const box = h('input', { class: 'switch', attrs: { type: 'checkbox' } });
  box.checked = preferences()[field];
  box.addEventListener('change', () => save({ [field]: box.checked }));
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
    setting('settings.general.thumbnails', checkbox('downloadThumbnails')),
    setting('settings.general.includeIds', checkbox('includeIdInTitleName')),
  ];
}

function interfaceTab(): HTMLElement[] {
  return [
    setting('settings.interface.theme', null, themePicker()),
    setting('settings.interface.playerPlacement', select([
      ['side', t('player.placement.side')], ['bottom', t('player.placement.bottom')], ['both', t('player.placement.both')],
    ], ui.playerPlacement, value => choosePlayerPlacement(value as PlayerPlacement))),
    // The whole app is drawn again in the new language, this screen included.
    setting('settings.interface.language', select(LANGUAGES, language(), chooseLanguage)),
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
  const button = h('button', { class: 'primary-button', onclick: () => void checkForUpdate() });
  const status = h('p', { class: 'help' });
  const show = () => {
    const { pending, busy, status: line, failed } = updateState();
    button.textContent = pending ? t('settings.updates.downloadAndInstall', pending.version)
      : t(failed ? 'settings.updates.checkAgain' : 'settings.updates.checkNow');
    button.disabled = busy;
    // Hidden while it has nothing to say: an empty line still takes its height.
    status.textContent = line;
    status.hidden = !line;
  };
  show();
  const stop = onUpdateChange(() => {
    if (!button.isConnected) {
      stop();
      return;
    }
    show();
  });
  return [
    h('section', { class: 'card settings-card' },
      h('div', { class: 'section-title', text: t('settings.updates.currentVersion') }),
      h('p', { class: 'subtitle', text: appVersion() })),
    h('section', { class: 'card settings-card' },
      h('div', { class: 'section-title', text: t('settings.updates.title') }),
      h('p', { class: 'subtitle', text: t('settings.updates.check.desc') }),
      h('div', { class: 'stack-6' }, h('div', {}, button), status)),
    setting('settings.updates.auto', checkbox('appUpdateCheckEnabled')),
  ];
}

// === HERRAMIENTAS ===

/** How many tools are missing, on the tab itself: visible from any other tab. */
function missingBadge(): HTMLElement {
  const badge = h('span', { class: 'tab-badge' });
  const show = () => {
    const count = missingTools(tools.report).length;
    badge.textContent = String(count);
    badge.hidden = count === 0;
  };
  subscribeWhile(badge, 'tools', show);
  show();
  return badge;
}

function toolsTab(): HTMLElement[] {
  const installMissing = h('button', { class: 'primary-button', text: t('settings.tools.installMissing'), onclick: () =>
    void installTools(missingTools(tools.report).map(tool => tool.id)) });
  const actions = h('div', { class: 'row center wrap gap-10' }, installMissing,
    h('button', { class: 'secondary-button', text: t('settings.tools.openFolder'), onclick: () => void api.openToolsFolder() }));
  const rows = h('div', { class: 'stack-12' });
  const render = () => {
    const report = tools.report;
    const missing = missingTools(report);
    installMissing.hidden = missing.length === 0;
    installMissing.disabled = missing.every(tool => isBusy(tool.id));
    actions.hidden = !report?.managed;
    if (!report) rows.replaceChildren(h('p', { class: 'help', text: t('settings.tools.loading') }));
    else if (!report.managed) rows.replaceChildren(h('section', { class: 'card settings-card' }, h('p', { class: 'subtitle', text: t('settings.tools.system') })));
    else rows.replaceChildren(...report.tools.map(toolCard));
  };
  subscribeWhile(rows, 'tools', render);
  render();
  // Asked again each time the tab opens: a tool may have been replaced by hand.
  void loadTools().catch(() => {});
  return [
    actions,
    rows,
    setting('settings.tools.autoUpdates', checkbox('autoUpdates')),
    h('p', { class: 'help', text: t('settings.tools.note') }),
  ];
}

function toolCard(tool: ToolStatus): HTMLElement {
  const busy = isBusy(tool.id);
  const status = h('p', { class: 'help' });
  if (busy) {
    const chip = toolChip(tool, tools);
    status.textContent = t(chip.key, ...chip.args);
  } else if (tools.failed.has(tool.id)) {
    status.textContent = t('settings.tools.failed');
    status.classList.add('status-text-warn');
  } else if (tool.installed) {
    const newer = tool.id === 'ytdlp' && tools.newerYtdlp ? ` · ${t('settings.tools.newer', tools.newerYtdlp)}` : '';
    status.textContent = t('settings.tools.installed', tool.version ?? '?') + newer;
    status.classList.add('status-text-ok');
  } else {
    status.textContent = tool.bundled ? t('settings.tools.missingBundled') : t('settings.tools.missing', String(TOOL_SIZES_MB[tool.id]));
    status.classList.add('status-text-warn');
  }

  const button = (key: string, primary: boolean, run: () => Promise<unknown>) => {
    const el = h('button', { class: primary ? 'primary-button' : 'secondary-button', text: t(key) });
    el.disabled = busy;
    el.addEventListener('click', () => {
      el.disabled = true;
      void run().catch(() => {}).finally(() => { el.disabled = isBusy(tool.id); });
    });
    return el;
  };
  const buttons: HTMLElement[] = [];
  if (!tool.installed) {
    buttons.push(button('settings.tools.install', true, () => installTools([tool.id])));
  } else {
    if (tool.id === 'ytdlp') {
      buttons.push(tools.newerYtdlp
        ? button('settings.tools.update', true, () => installTools(['ytdlp'], true))
        : button('settings.tools.checkUpdate', false, checkYtdlp));
    }
    buttons.push(button('settings.tools.reinstall', false, () => installTools([tool.id], true)));
  }
  return h('section', { class: 'card settings-card', attrs: { 'data-tool': tool.id } },
    h('div', { class: 'row center gap-14' },
      h('div', { class: 'grow' },
        h('div', { class: 'section-title', text: tool.name + (tool.required ? '' : ` · ${t('settings.tools.recommended')}`) }),
        h('p', { class: 'subtitle', text: t(`tools.${tool.id}.desc`) }),
        status),
      h('div', { class: 'row center gap-8' }, ...buttons)));
}

function advanced(): HTMLElement[] {
  const path = h('input', { class: 'input grow', attrs: { readonly: '', placeholder: t('settings.advanced.cookies.placeholder') } });
  const status = h('p', { class: 'help' });
  const show = (file: string) => {
    path.value = file;
    void showCookiesStatus(status, file);
  };
  const choose = async () => {
    const file = await api.pickCookiesFile(t('settings.cookies.chooseFile'), t('settings.cookies.fileFilter'));
    if (file === null) return;
    save({ cookiesFile: file });
    show(file);
  };
  show(preferences().cookiesFile);
  return [
    setting('settings.advanced.cookies', null, h('div', { class: 'stack-6' },
      h('div', { class: 'row center gap-10' }, path,
        h('button', { class: 'secondary-button', text: t('common.select'), onclick: () => void choose() }),
        h('button', { class: 'secondary-button', text: t('common.remove'), onclick: () => { save({ cookiesFile: '' }); show(''); } })),
      status)),
    setting('settings.advanced.reset', h('button', { class: 'danger-button', text: t('settings.advanced.reset.button'), onclick: () =>
      confirmDialog(t('settings.advanced.reset.confirm.title'), t('settings.advanced.reset.confirm.header'), t('settings.advanced.reset.confirm.body'), [
        // Like the Java app: a reset is applied on the spot (theme, language,
        // scale), not only written down — otherwise the button seemed to do nothing.
        { label: t('settings.advanced.reset.button'), kind: 'danger', run: () => void resetAndRedraw() },
      ]) })),
  ];
}

async function resetAndRedraw(): Promise<void> {
  await resetPreferences();
  // This panel's own controls (the cookies path) still show the old values
  // unless the screen is drawn again.
  navigate('settings');
  notify({ tone: 'success', text: t('settings.advanced.reset.done') });
}

/**
 * The cookies status line, with its colour: green if the file is there, amber
 * if a path is saved but nothing is at the end of it (that one needs fixing),
 * muted if there are simply no cookies — not a failure, just the 100-files
 * limit. It is a state, not a notice: it does not expire.
 */
async function showCookiesStatus(line: HTMLElement, file: string): Promise<void> {
  line.classList.remove('status-text-ok', 'status-text-warn');
  if (!file.trim()) {
    line.textContent = `✕ ${t('settings.cookies.none')}`;
    return;
  }
  const exists = await api.fileExists(file);
  line.textContent = t(exists ? 'settings.cookies.ok' : 'settings.cookies.missing');
  line.classList.add(exists ? 'status-text-ok' : 'status-text-warn');
}

function about(): HTMLElement[] {
  return [
    h('section', { class: 'card settings-card' },
      h('div', { class: 'row center gap-14' },
        h('img', { attrs: { src: '/icons/soundify-logo.png', alt: '', width: '40', height: '40' } }),
        h('div', { class: 'grow' },
          h('div', { class: 'section-title', text: 'Soundify' }),
          h('p', { class: 'subtitle', text: t('app.description') })),
        h('span', { class: 'chip', text: appVersion() }))),
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
