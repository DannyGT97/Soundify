import { expect, test } from '@playwright/test';

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

const SCREENS = ['home', 'download', 'new-playlist', 'manager', 'player', 'now-playing', 'settings'];

for (const screen of SCREENS) {
  test(`${screen} fits a phone without scrolling sideways`, async ({ page }) => {
    await page.goto(`/#/${screen}`);
    await expect(page.locator('.screen')).toBeVisible();
    const widths = await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
    expect(widths[0]).toBe(widths[1]);
  });
}

test('the menu is a tab bar at the bottom, with one tab for both ways of downloading', async ({ page }) => {
  await page.goto('/#/home');
  const bar = page.locator('.sidebar');
  await expect(page.locator('.sidebar-header')).toBeHidden();
  await expect(bar.getByRole('button')).toHaveCount(5);
  const box = await bar.boundingBox();
  expect(box!.y + box!.height).toBe(844);
  expect(box!.width).toBe(390);

  const downloads = bar.locator('.nav-button', { hasText: 'Descargar' });
  await downloads.click();
  await expect(page).toHaveURL(/#\/download/);
  await page.locator('.form-switch').getByRole('button', { name: 'Crear Playlist' }).click();
  await expect(page).toHaveURL(/#\/new-playlist/);
  await expect(downloads).toHaveClass(/phone-selected/);
});

test('the player bar is only cover, title, play, next and close; Reproduciendo has the rest', async ({ page }) => {
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track').first().click();
  const bar = page.locator('.player-bar');
  await expect(bar).toBeVisible();
  await expect(bar.locator('.player-play')).toBeVisible();
  await expect(bar.locator('.player-volume')).toBeHidden();
  await expect(bar.locator('.player-time').first()).toBeHidden();

  await bar.locator('.player-now').click();
  await expect(page).toHaveURL(/#\/now-playing/);
  await expect(bar).toBeHidden();
  await expect(page.locator('.np-controls .player-progress')).toBeVisible();
  await expect(page.locator('.np-controls .player-volume')).toBeHidden();
});

test('on Android what the back cannot do yet is neither offered nor clickable', async ({ page }) => {
  await page.goto('/?mockAndroid#/download');
  await expect(page.getByText('En Android todavía no se puede descargar')).toBeVisible();
  await expect(page.getByText('Dispositivo MTP')).toBeHidden();
  await expect(page.locator('.screen').getByRole('button', { name: 'Descargar' })).toBeDisabled();

  await page.goto('/?mockAndroid#/manager');
  await expect(page.getByRole('button', { name: 'Importar' })).toBeHidden();

  await page.goto('/?mockAndroid#/settings');
  await expect(page.locator('.settings-tab')).toHaveCount(4);
  await expect(page.getByRole('button', { name: 'Herramientas' })).toBeHidden();
  await page.getByPlaceholder('Buscar ajustes').fill('yt-dlp');
  await expect(page.locator('.search-hit').filter({ hasText: 'Herramientas' })).toHaveCount(0);
});
