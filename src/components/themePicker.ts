import { h } from '../dom';
import { t } from '../i18n';
import { setTheme, type Theme, ui } from '../state';

const THEMES: { id: Theme; label: string; official: boolean }[] = [
  { id: 'dark', label: 'theme.dark', official: true },
  { id: 'dark-red', label: 'theme.darkRed', official: false },
  { id: 'dark-blue', label: 'theme.darkBlue', official: false },
  { id: 'amoled', label: 'theme.amoled', official: false },
  { id: 'light', label: 'theme.light', official: true },
  { id: 'light-red', label: 'theme.lightRed', official: false },
  { id: 'light-blue', label: 'theme.lightBlue', official: false },
];

export function themePicker(): HTMLElement {
  const grid = h('div', { class: 'theme-picker', attrs: { role: 'radiogroup', 'aria-label': t('settings.interface.theme') } });
  const refresh = () => grid.querySelectorAll<HTMLButtonElement>('.theme-option').forEach(button => {
    const selected = button.dataset.theme === ui.theme;
    button.classList.toggle('selected', selected);
    button.setAttribute('aria-checked', String(selected));
  });
  for (const theme of THEMES) {
    const sample = h('div', { class: 'theme-swatch', attrs: { 'data-theme': theme.id } }, h('i'), h('i'));
    const label = h('span', { class: 'theme-option-label' }, h('span', { text: t(theme.label) }), theme.official ? h('small', { text: t('theme.official') }) : null);
    const button = h('button', { class: 'theme-option', attrs: { role: 'radio', 'data-theme': theme.id } }, sample, label);
    button.addEventListener('click', () => { setTheme(theme.id); refresh(); });
    grid.append(button);
  }
  refresh();
  return grid;
}
