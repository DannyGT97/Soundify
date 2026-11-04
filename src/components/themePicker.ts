import { h } from '../dom';
import { t } from '../i18n';
import { type Theme, ui } from '../state';
import { chooseTheme } from '../preferences';

const THEMES: { id: Theme; label: string }[] = [
  { id: 'dark', label: 'theme.dark' },
  { id: 'dark-neutral', label: 'theme.darkNeutral' },
  { id: 'dark-red', label: 'theme.darkRed' },
  { id: 'dark-blue', label: 'theme.darkBlue' },
  { id: 'amoled', label: 'theme.amoled' },
  { id: 'light', label: 'theme.light' },
  { id: 'light-neutral', label: 'theme.lightNeutral' },
  { id: 'light-red', label: 'theme.lightRed' },
  { id: 'light-blue', label: 'theme.lightBlue' },
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
    const label = h('span', { class: 'theme-option-label' }, h('span', { text: t(theme.label) }));
    const button = h('button', { class: 'theme-option', attrs: { role: 'radio', 'data-theme': theme.id } }, sample, label);
    button.addEventListener('click', () => { chooseTheme(theme.id); refresh(); });
    grid.append(button);
  }
  refresh();
  return grid;
}
