// Draws the icons of the buttons that Windows shows under the preview of the app's window in the taskbar (previous, play/pause,
// next, shuffle, repeat): Lucide icons (the same as the app), as 32×32 PNGs with a transparent background, in two sets: light glyphs
// for the dark taskbar and dark glyphs for the light one. `src-tauri/src/thumbbar.rs` embeds them.
//
//   node scripts/make-thumbbar-icons.mjs
//
// Needs the browser of the interface tests (npx playwright install chromium). The PNGs are in git: this only has to run when an icon changes.

import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { Pause, Play, Repeat, Repeat1, Shuffle, SkipBack, SkipForward } from 'lucide';

const OUT = join(import.meta.dirname, '..', 'src-tauri', 'icons', 'thumbbar');
const ACCENT = '#1ed760';

/** What each button shows. `fill`: the glyph is solid (play, pause and the skips) instead of a line drawing. */
const ICONS = [
  { name: 'previous', node: SkipBack, fill: true },
  { name: 'next', node: SkipForward, fill: true },
  { name: 'play', node: Play, fill: true },
  { name: 'pause', node: Pause, fill: true },
  { name: 'shuffle-off', node: Shuffle },
  { name: 'shuffle-on', node: Shuffle, accent: true },
  { name: 'repeat-off', node: Repeat },
  { name: 'repeat-all', node: Repeat, accent: true },
  { name: 'repeat-one', node: Repeat1, accent: true },
];

const THEMES = { dark: '#ffffff', light: '#1b1b1b' };

function svg(node, color, fill) {
  const parts = node.map(([tag, attributes]) => `<${tag} ${Object.entries(attributes).map(([key, value]) => `${key}="${value}"`).join(' ')}/>`).join('');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="${fill ? color : 'none'}" stroke="${color}" stroke-width="${fill ? 1.6 : 2.2}" stroke-linecap="round" stroke-linejoin="round">${parts}</svg>`;
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 32, height: 32 }, deviceScaleFactor: 1 });
for (const [theme, base] of Object.entries(THEMES)) {
  mkdirSync(join(OUT, theme), { recursive: true });
  for (const icon of ICONS) {
    const color = icon.accent ? ACCENT : base;
    await page.setContent(`<html><body style="margin:0;background:transparent">${svg(icon.node, color, icon.fill)}</body></html>`);
    const png = await page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: 32, height: 32 } });
    writeFileSync(join(OUT, theme, `${icon.name}.png`), png);
  }
}
await browser.close();
console.log(`${ICONS.length * Object.keys(THEMES).length} icons in ${OUT}`);
