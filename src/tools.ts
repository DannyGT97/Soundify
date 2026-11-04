// The download tools (yt-dlp, ffmpeg, Deno) as the screens see them: which are
// there, which are being installed and how far along. The Java app installed
// all three by itself at startup (~180 MB without asking) and said nothing
// until a download failed; now the first-start assistant, Ajustes →
// Herramientas and Descarga's notice read this one state and install through it.

import { api } from './api';
import type { ApiError, ToolDownload, ToolId, ToolsReport, ToolStatus, YtdlpCheck } from './api/types';
import { t } from './i18n';
import { navigate } from './router';
import { emit, subscribe, ui } from './state';
import { liveNotice, notify } from './components/toasts';
import { onAndroid } from './platform';
import { messageText } from './tasks';
import { toolDownloadsText } from './toolDownloads';

/** What each one weighs to download, roughly: the assistant adds them up before asking. */
export const TOOL_SIZES_MB: Record<ToolId, number> = { ytdlp: 18, ffmpeg: 115, deno: 45 };

export interface ToolsState {
  report: ToolsReport | null;
  /** Waiting their turn: they install one after the other. */
  queued: Set<ToolId>;
  installing: Set<ToolId>;
  failed: Set<ToolId>;
  downloads: ToolDownload[];
  /** A newer yt-dlp found by «Buscar actualización». */
  newerYtdlp: string | null;
  /** The assistant shows the progress itself: the notice stays away meanwhile. */
  wizardOpen: boolean;
}

export const tools: ToolsState = {
  report: null,
  queued: new Set(),
  installing: new Set(),
  failed: new Set(),
  downloads: [],
  newerYtdlp: null,
  wizardOpen: false,
};

// === PURE LOGIC ===

/** Missing tools, in order. Outside Windows the system's are used: nothing is ever missing. */
export function missingTools(report: ToolsReport | null): ToolStatus[] {
  if (!report?.managed) return [];
  return report.tools.filter(tool => !tool.installed);
}

/** The missing ones that stop every download (Deno is only recommended). */
export function missingRequired(report: ToolsReport | null): ToolStatus[] {
  return missingTools(report).filter(tool => tool.required);
}

/**
 * Can anything be downloaded or created? The one question every screen asks
 * (`status::can_download` is the same one in the back, which has the last word).
 * Before the report arrives it says yes: the back still refuses if it must.
 */
export function canDownload(report: ToolsReport | null): boolean {
  if (onAndroid) return false;
  return missingRequired(report).length === 0;
}

/** "≈ 178 MB" for what has to come from the internet; a copy that travels with the app weighs nothing. */
export function downloadSize(missing: readonly ToolStatus[]): number {
  return missing.filter(tool => !tool.bundled).reduce((sum, tool) => sum + TOOL_SIZES_MB[tool.id], 0);
}

export type ChipTone = 'neutral' | 'busy' | 'ok' | 'warn' | 'error';
export interface Chip { key: string; args: string[]; tone: ChipTone }

/** The chip beside each tool: what it is doing, or how it is. */
export function toolChip(tool: ToolStatus, state: Pick<ToolsState, 'queued' | 'installing' | 'failed' | 'downloads'>): Chip {
  const download = state.downloads.find(d => d.id === tool.id);
  if (download) {
    return download.percent === null
      ? { key: 'tools.chip.downloadingUnknown', args: [], tone: 'busy' }
      : { key: 'tools.chip.downloading', args: [String(download.percent)], tone: 'busy' };
  }
  if (state.installing.has(tool.id)) return { key: tool.bundled ? 'tools.chip.copying' : 'tools.chip.installing', args: [], tone: 'busy' };
  if (state.queued.has(tool.id)) return { key: 'tools.chip.queued', args: [], tone: 'neutral' };
  if (state.failed.has(tool.id)) return { key: 'tools.chip.failed', args: [], tone: 'error' };
  if (tool.installed) return { key: 'tools.chip.ready', args: [tool.version ?? '?'], tone: 'ok' };
  if (tool.bundled) return { key: 'tools.chip.bundled', args: [], tone: 'neutral' };
  return { key: 'tools.chip.missing', args: [String(TOOL_SIZES_MB[tool.id])], tone: 'warn' };
}

// === STATE ===

/** Asks each tool again (versions included): after an install, and when Herramientas opens. */
export async function loadTools(): Promise<void> {
  tools.report = await api.toolsReport();
  emit('tools');
}

export function isBusy(id: ToolId): boolean {
  return tools.queued.has(id) || tools.installing.has(id);
}

/**
 * Installs `ids` one after the other (the ones already on their way are
 * skipped). Resolves true if all of them made it. A failure outside the
 * assistant leaves a notice with «Reintentar»; inside it, the assistant says it.
 */
export async function installTools(ids: readonly ToolId[], reinstall = false): Promise<boolean> {
  const todo = ids.filter(id => !isBusy(id));
  todo.forEach(id => { tools.queued.add(id); tools.failed.delete(id); });
  emit('tools');
  let allOk = true;
  for (const id of todo) {
    tools.queued.delete(id);
    tools.installing.add(id);
    emit('tools');
    try {
      await api.installTool(id, reinstall);
      if (id === 'ytdlp') tools.newerYtdlp = null;
    } catch (error) {
      allOk = false;
      tools.failed.add(id);
      if (!tools.wizardOpen) reportFailure(id, error as ApiError);
    } finally {
      tools.installing.delete(id);
    }
    await loadTools().catch(() => emit('tools'));
  }
  return allOk;
}

/** «Buscar actualización» of yt-dlp: says it in a notice when there is nothing new. */
export async function checkYtdlp(): Promise<YtdlpCheck> {
  const check = await api.checkYtdlpUpdate();
  tools.newerYtdlp = check.newer ? check.latest : null;
  emit('tools');
  if (check.latest === null) notify({ tone: 'error', text: t('tools.check.failed') });
  else if (!check.newer) notify({ tone: 'success', text: t('tools.check.upToDate', 'yt-dlp') });
  return check;
}

/** Ajustes on the Herramientas tab. */
export function openToolsSettings(): void {
  ui.settingsTab = 'tools';
  navigate('settings');
}

/**
 * Follows the downloads from the back, and keeps one notice with their
 * percentage while the assistant is not on screen: on a first start ffmpeg is
 * ~115 MB from a slow server, and with nothing visible the app looked stuck.
 */
export function watchTools(): void {
  let notice: ReturnType<typeof liveNotice> | null = null;
  api.watchToolDownloads(downloads => {
    tools.downloads = downloads;
    emit('tools');
  });
  subscribe('tools', () => {
    const text = tools.wizardOpen ? '' : toolDownloadsText(tools.downloads);
    if (!text) {
      notice?.dismiss();
      notice = null;
    } else if (notice) {
      notice.setText(text);
    } else {
      notice = liveNotice('neutral', text, { label: t('tools.notice.view'), run: openToolsSettings });
    }
  });
}

function reportFailure(id: ToolId, error: ApiError): void {
  notify({
    tone: 'error',
    persistent: true,
    text: messageText(error.key, error.args),
    action: { label: t('tools.retry'), run: () => void installTools([id]) },
  });
}
