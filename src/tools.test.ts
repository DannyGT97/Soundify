import { describe, expect, it } from 'vitest';
import type { ToolStatus, ToolsReport } from './api/types';
import { canDownload, downloadSize, missingRequired, missingTools, toolChip } from './tools';

const tool = (id: ToolStatus['id'], installed: boolean, extra: Partial<ToolStatus> = {}): ToolStatus =>
  ({ id, name: id, installed, version: installed ? '1.0' : null, bundled: false, required: id !== 'deno', ...extra });
const report = (managed: boolean, tools: ToolStatus[], builtIn = false): ToolsReport => ({ managed, builtIn, folder: 'data/systemtools', tools });
const idle = { queued: new Set<ToolStatus['id']>(), installing: new Set<ToolStatus['id']>(), failed: new Set<ToolStatus['id']>(), downloads: [] };

describe('missing tools', () => {
  it('lists what is missing, and only yt-dlp and ffmpeg block downloads', () => {
    const r = report(true, [tool('ytdlp', false), tool('ffmpeg', true), tool('deno', false)]);
    expect(missingTools(r).map(x => x.id)).toEqual(['ytdlp', 'deno']);
    expect(missingRequired(r).map(x => x.id)).toEqual(['ytdlp']);
  });

  it('nothing is missing when the system provides the tools, or before the report arrives', () => {
    expect(missingTools(report(false, [tool('ytdlp', false)]))).toEqual([]);
    expect(missingRequired(null)).toEqual([]);
  });

  it('downloading needs yt-dlp and ffmpeg, and Deno does not stop it', () => {
    const all = (ytdlp: boolean, ffmpeg: boolean, deno: boolean) => report(true, [tool('ytdlp', ytdlp), tool('ffmpeg', ffmpeg), tool('deno', deno)]);
    expect(canDownload(all(true, true, true))).toBe(true);
    expect(canDownload(all(true, true, false))).toBe(true);
    expect(canDownload(all(false, true, true))).toBe(false);
    expect(canDownload(all(true, false, true))).toBe(false);
  });

  it('it says yes when the system provides the tools, or before the report arrives', () => {
    expect(canDownload(report(false, [tool('ytdlp', false)]))).toBe(true);
    expect(canDownload(null)).toBe(true);
  });

  it('the tools in the APK block downloads only while they are being unpacked', () => {
    const unpacking = report(false, [tool('ytdlp', false), tool('ffmpeg', false), tool('deno', false)], true);
    expect(missingRequired(unpacking).map(x => x.id)).toEqual(['ytdlp', 'ffmpeg']);
    expect(canDownload(unpacking)).toBe(false);
    const ready = report(false, [tool('ytdlp', true), tool('ffmpeg', true), tool('deno', true)], true);
    expect(canDownload(ready)).toBe(true);
  });

  it('a copy travelling with the app adds nothing to the download size', () => {
    expect(downloadSize([tool('ytdlp', false), tool('ffmpeg', false, { bundled: true }), tool('deno', false)])).toBe(18 + 45);
  });
});

describe('toolChip', () => {
  it('the download in progress wins over everything else, with or without a percentage', () => {
    const state = { ...idle, installing: new Set(['ffmpeg'] as const), downloads: [{ id: 'ffmpeg' as const, tool: 'ffmpeg', percent: 45 }] };
    expect(toolChip(tool('ffmpeg', false), state)).toEqual({ key: 'tools.chip.downloading', args: ['45'], tone: 'busy' });
    state.downloads[0]!.percent = null as unknown as number;
    expect(toolChip(tool('ffmpeg', false), state).key).toBe('tools.chip.downloadingUnknown');
  });

  it('installing without a download is a copy for the bundled ffmpeg and an install otherwise', () => {
    const state = { ...idle, installing: new Set(['ffmpeg'] as const) };
    expect(toolChip(tool('ffmpeg', false, { bundled: true }), state).key).toBe('tools.chip.copying');
    expect(toolChip(tool('ffmpeg', false), state).key).toBe('tools.chip.installing');
  });

  it('queued, failed, ready, bundled and missing', () => {
    expect(toolChip(tool('deno', false), { ...idle, queued: new Set(['deno'] as const) }).key).toBe('tools.chip.queued');
    expect(toolChip(tool('deno', false), { ...idle, failed: new Set(['deno'] as const) }).tone).toBe('error');
    expect(toolChip(tool('deno', true), idle)).toEqual({ key: 'tools.chip.ready', args: ['1.0'], tone: 'ok' });
    expect(toolChip(tool('ffmpeg', false, { bundled: true }), idle).key).toBe('tools.chip.bundled');
    expect(toolChip(tool('ffmpeg', false), idle)).toEqual({ key: 'tools.chip.missing', args: ['115'], tone: 'warn' });
  });
});
