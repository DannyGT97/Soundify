import { expect, test } from '@playwright/test';

// The webview's zoom lays the page out again in fewer CSS pixels: a window of 1280×820 at 150 % is a page of 853×547. Nothing may spill
// sideways at any scale, in the window the app opens with and in the smallest one it allows (900×600). Checked as the page sees it.

const WINDOWS = [{ width: 1280, height: 820 }, { width: 900, height: 600 }];
const SCALES = [80, 100, 125, 150, 175];
const ROUTES = ['#/home', '#/download', '#/new-playlist', '#/manager?open=pl1', '#/player', '#/player?b=pl1%3AMP3', '#/now-playing', '#/settings'];

for (const window of WINDOWS) {
  for (const scale of SCALES) {
    test(`nothing spills sideways at ${scale}% in a ${window.width}×${window.height} window`, async ({ page }) => {
      await page.setViewportSize({ width: Math.round(window.width / (scale / 100)), height: Math.round(window.height / (scale / 100)) });
      const problems: string[] = [];
      for (const route of ROUTES) {
        await page.goto(`/?mockConnect${route}`);
        await page.locator('.screen').first().waitFor();
        const found = await page.evaluate(() => {
          const bad: string[] = [];
          const root = document.documentElement;
          if (root.scrollWidth > root.clientWidth + 1) bad.push(`page ${root.scrollWidth}>${root.clientWidth}`);
          for (const selector of ['.screen', '.sidebar', '.player-bar', '.detail']) {
            const element = document.querySelector<HTMLElement>(selector);
            if (element && element.scrollWidth > element.clientWidth + 1) bad.push(`${selector} ${element.scrollWidth}>${element.clientWidth}`);
          }
          return bad;
        });
        if (found.length) problems.push(`${route}: ${found.join(', ')}`);
      }
      expect(problems).toEqual([]);
    });
  }
}
