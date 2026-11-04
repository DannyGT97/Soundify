// The tools in a plain browser: all in place, unless the page is opened with
// `?mockTools=missing` (the first-start assistant and Ajustes → Herramientas
// then have something to install) or `?mockTools=failing` (the same, and
// Deno's download fails, to see the retry). Installing is a percentage
// climbing on a timer, reported like the real one.

import type { ToolDownload, ToolId, ToolsReport, ToolStatus, YtdlpCheck } from '../types';

const STEP_MS = 120;
const mode = typeof location === 'undefined' ? '' : new URLSearchParams(location.search).get('mockTools') ?? '';

const VERSIONS: Record<ToolId, string> = { ytdlp: '2026.10.22', ffmpeg: '7.1', deno: '2.5.4' };
const tools: ToolStatus[] = [
  { id: 'ytdlp', name: 'yt-dlp', installed: true, version: VERSIONS.ytdlp, bundled: false, required: true },
  { id: 'ffmpeg', name: 'ffmpeg', installed: true, version: VERSIONS.ffmpeg, bundled: false, required: true },
  { id: 'deno', name: 'Deno', installed: true, version: VERSIONS.deno, bundled: false, required: false },
];
if (mode) {
  for (const tool of tools) {
    tool.installed = false;
    tool.version = null;
  }
}

const downloads = new Map<ToolId, ToolDownload>();
let watcher: (downloads: ToolDownload[]) => void = () => {};
let denoFailures = mode === 'failing' ? 1 : 0;

/** The back's `status::can_download`: the mock refuses too, whatever the interface lets through. */
export function mockCanDownload(): boolean {
  return tools.every(tool => tool.installed || !tool.required);
}

export const mockTools = {
  toolsReport: async (): Promise<ToolsReport> => ({ managed: true, folder: 'data\systemtools', tools: tools.map(tool => ({ ...tool })) }),
  installTool,
  checkYtdlpUpdate: async (): Promise<YtdlpCheck> => ({ latest: '2026.11.01', newer: true }),
  openToolsFolder: async () => {},
  watchToolDownloads: (onChange: (downloads: ToolDownload[]) => void) => {
    watcher = onChange;
    onChange([...downloads.values()]);
  },
};

async function installTool(id: ToolId, _reinstall: boolean): Promise<void> {
  const tool = tools.find(x => x.id === id)!;
  if (downloads.has(id)) throw { key: 'tools.error.busy', args: [] };
  const failAt = id === 'deno' && denoFailures > 0 ? 40 : null;
  for (let percent = 0; percent <= 100; percent += 10) {
    downloads.set(id, { id, tool: tool.name, percent });
    watcher([...downloads.values()]);
    await new Promise(resolve => setTimeout(resolve, STEP_MS));
    if (percent === failAt) {
      denoFailures--;
      downloads.delete(id);
      watcher([...downloads.values()]);
      throw { key: 'tools.error.failed', args: [tool.name, 'connection reset'] };
    }
  }
  downloads.delete(id);
  watcher([...downloads.values()]);
  tool.installed = true;
  tool.version = id === 'ytdlp' ? '2026.11.01' : VERSIONS[id];
}
