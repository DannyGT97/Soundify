// The first-start assistant: what the three tools are, what is missing and
// how much it weighs, and installing it with the user watching. It opens by
// itself only when a REQUIRED tool is missing; closing it is fine — the app
// keeps working, Descarga says what is missing and Ajustes → Herramientas
// installs it later.

import { h } from '../dom';
import { t } from '../i18n';
import { closeOverlay, openOverlay } from '../components/overlay';
import { notify } from '../components/toasts';
import { emit, subscribeWhile } from '../state';
import { downloadSize, installTools, missingRequired, missingTools, openToolsSettings, toolChip, tools } from '../tools';
import type { ToolStatus } from '../api/types';

type Step = 'intro' | 'installing' | 'done' | 'failed';

export function openToolsSetup(): void {
  let step: Step = 'intro';
  const title = h('h2', { class: 'overlay-title' });
  const text = h('p', { class: 'help' });
  const list = h('div', { class: 'stack-6 tools-list' });
  const summary = h('p', { class: 'help' });
  const actions = h('div', { class: 'row wrap end center gap-8' });

  // The step follows the state, not the click: opened from Descarga while an
  // install already runs (started in Ajustes), it shows that one going.
  const install = () => void installTools(missingTools(tools.report).map(tool => tool.id));

  const render = () => {
    const busy = [...tools.queued, ...tools.installing].length > 0;
    if (busy) step = 'installing';
    if (!busy && step === 'installing') step = tools.failed.size ? 'failed' : 'done';
    const missing = missingTools(tools.report);
    title.textContent = t(`tools.setup.${step}.title`);
    text.textContent = t(step === 'failed' ? 'tools.setup.failed.text' : step === 'intro' ? 'tools.setup.intro' : 'tools.setup.background');
    list.replaceChildren(...(tools.report?.tools ?? []).map(toolRow));
    summary.hidden = step !== 'intro';
    summary.textContent = missing.length
      ? t('tools.setup.summary', String(missing.length), String(downloadSize(missing)))
      : t('tools.setup.allReady');
    actions.replaceChildren(...buttons(missing.length > 0));
  };

  const buttons = (anythingMissing: boolean): HTMLElement[] => {
    switch (step) {
      case 'intro':
        return anythingMissing
          ? [h('span', { class: 'help grow', text: t('tools.setup.laterHint') }),
             h('button', { class: 'secondary-button', text: t('tools.setup.later'), onclick: () => closeOverlay() }),
             h('button', { class: 'primary-button', text: t('tools.setup.install'), onclick: install })]
          : [h('button', { class: 'primary-button', text: t('tools.setup.start'), onclick: () => closeOverlay() })];
      case 'installing':
        return [h('button', { class: 'secondary-button', text: t('tools.setup.background.button'), onclick: () => closeOverlay() })];
      case 'done':
        return [h('button', { class: 'primary-button', text: t('tools.setup.start.done'), onclick: () => closeOverlay() })];
      case 'failed':
        return [h('button', { class: 'secondary-button', text: t('tools.setup.background.button'), onclick: () => closeOverlay() }),
                h('button', { class: 'primary-button', text: t('tools.retry'), onclick: install })];
    }
  };

  tools.wizardOpen = true;
  openOverlay(title, [text, list, summary, actions], {
    onClose: () => {
      tools.wizardOpen = false;
      // The progress goes on in the notice from here.
      emit('tools');
      // «Ahora no»: without the required ones nothing downloads; say where to come back.
      if (step === 'intro' && missingRequired(tools.report).length) {
        notify({ tone: 'warning', persistent: true, text: t('tools.later.notice'),
          action: { label: t('tools.later.action'), run: openToolsSettings } });
      }
    },
  });
  subscribeWhile(list, 'tools', render);
  render();
}

function toolRow(tool: ToolStatus): HTMLElement {
  const chip = toolChip(tool, tools);
  return h('div', { class: 'tools-row' },
    h('div', { class: 'grow' },
      h('div', { class: 'row-title', text: tool.name }),
      h('p', { class: 'subtitle', text: t(`tools.${tool.id}.desc`) })),
    h('span', { class: `chip tools-chip tools-chip-${chip.tone}`, text: t(chip.key, ...chip.args) }));
}
