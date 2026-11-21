import { expect, test } from '@playwright/test';

// The media keys: what plays is told to the system, and a key does what the bar's buttons do.

type Media = { updates: Array<{ active: boolean; title: string; playing: boolean; shuffle: boolean; repeat: string }>; press: (action: string, value?: string) => void };
const media = (page: import('@playwright/test').Page) => page.evaluate(() => (window as unknown as { __media: Media }).__media.updates.at(-1));
const press = (page: import('@playwright/test').Page, action: string, value?: string) =>
  page.evaluate(([a, v]) => (window as unknown as { __media: Media }).__media.press(a as string, v as string | undefined), [action, value]);

test('the system is told what plays and in which state', async ({ page }) => {
  await page.goto('/#/player?b=pl1:MP3');
  expect((await media(page))?.active).toBe(false);
  await page.locator('.tracks .track-row:not(.off)').first().click();
  await expect.poll(async () => (await media(page))?.playing).toBe(true);
  expect((await media(page))?.title).not.toBe('');
  await page.getByRole('button', { name: 'Aleatorio' }).first().click();
  await expect.poll(async () => (await media(page))?.shuffle).toBe(true);
});

test('the keys play, pause, skip and go back like the bar does', async ({ page }) => {
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.tracks .track-row:not(.off)').first().click();
  const title = () => page.locator('.player-bar .player-now .row-title').textContent();
  const first = await title();
  await press(page, 'pause');
  await expect.poll(async () => (await media(page))?.playing).toBe(false);
  await press(page, 'play');
  await expect.poll(async () => (await media(page))?.playing).toBe(true);
  await press(page, 'next');
  await expect.poll(title).not.toBe(first);
  await press(page, 'previous');
  await expect.poll(title).toBe(first);
});

test('shuffle and repeat asked by the system end up as asked, whatever they were', async ({ page }) => {
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.tracks .track-row:not(.off)').first().click();
  await press(page, 'shuffle', 'true');
  await expect.poll(async () => (await media(page))?.shuffle).toBe(true);
  // Asked again for what it already is: nothing flips.
  await press(page, 'shuffle', 'true');
  await expect.poll(async () => (await media(page))?.shuffle).toBe(true);
  await press(page, 'shuffle', 'false');
  await expect.poll(async () => (await media(page))?.shuffle).toBe(false);
  await press(page, 'repeat', 'one');
  await expect.poll(async () => (await media(page))?.repeat).toBe('one');
  await press(page, 'repeat', 'all');
  await expect.poll(async () => (await media(page))?.repeat).toBe('all');
  await press(page, 'repeat', 'off');
  await expect.poll(async () => (await media(page))?.repeat).toBe('off');
});

test('when the player is closed the system session hides', async ({ page }) => {
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.tracks .track-row:not(.off)').first().click();
  await expect.poll(async () => (await media(page))?.active).toBe(true);
  await page.getByRole('button', { name: 'Cerrar el reproductor' }).first().click();
  await expect.poll(async () => (await media(page))?.active).toBe(false);
});
