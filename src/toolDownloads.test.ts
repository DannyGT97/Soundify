import { describe, expect, it } from 'vitest';
import { toolDownloadsText } from './toolDownloads';

describe('toolDownloadsText', () => {
  it('names each tool with its percent, or bare when the size is unknown', () => {
    const text = toolDownloadsText([{ id: 'ffmpeg', tool: 'ffmpeg', percent: 45 }, { id: 'deno', tool: 'Deno', percent: null }]);
    expect(text).toContain('ffmpeg (45 %), Deno');
  });

  it('says nothing once everything is downloaded', () => {
    expect(toolDownloadsText([])).toBe('');
  });
});
