// The notice while the app downloads its tools by itself: on a first start
// ffmpeg is ~115 MB from a slow server, and with nothing on screen the app
// looked stuck.

import type { ToolDownload } from './api/types';
import { t } from './i18n';

/** "ffmpeg (45 %)", "Deno" when the size is unknown; several joined. Empty list = nothing to say. */
export function toolDownloadsText(downloads: readonly ToolDownload[]): string {
  if (downloads.length === 0) return '';
  const tools = downloads.map(d => d.percent === null ? d.tool : `${d.tool} (${d.percent} %)`).join(', ');
  return t('toolDownloads.message', tools);
}
