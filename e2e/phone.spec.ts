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

test('on Android the tools come in the app, the music goes to Music/Soundify and only the MTP way is hidden', async ({ page }) => {
  await page.goto('/?mockAndroid#/download');
  await expect(page.locator('.status-strip.warning')).toBeHidden();
  await expect(page.getByText('Dispositivo MTP')).toBeHidden();
  await expect(page.locator('.dest', { hasText: '/storage/emulated/0/Music/Soundify' })).toBeVisible();

  await page.goto('/?mockAndroid#/manager');
  await expect(page.getByRole('button', { name: 'Importar' })).toBeVisible();

  await page.goto('/?mockAndroid#/settings');
  await expect(page.locator('.settings-tab')).toHaveCount(5);
  await expect(page.locator('.settings-tab', { hasText: 'Actualizaciones' })).toBeHidden();
  await page.getByPlaceholder('Buscar ajustes').fill('yt-dlp');
  await expect(page.locator('.search-hit').filter({ hasText: 'Actualizaciones' })).toHaveCount(0);
});

test('on Android, Herramientas offers to update yt-dlp (the one in the app ages) and nothing else', async ({ page }) => {
  await page.goto('/?mockAndroid#/settings');
  await page.locator('.settings-tab', { hasText: 'Herramientas' }).click();
  await expect(page.locator('[data-tool="ytdlp"]')).toContainText('2026.10.22');
  await expect(page.locator('[data-tool="ffmpeg"]')).toHaveCount(0);
  await expect(page.locator('[data-tool="deno"]')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Reinstalar' })).toHaveCount(0);
  await expect(page.getByText('Actualizar yt-dlp automáticamente')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Actualizar yt-dlp' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Buscar actualización' }).click();
  await expect(page.locator('[data-tool="ytdlp"]')).toContainText('2026.11.01');
  await page.getByRole('button', { name: 'Actualizar yt-dlp' }).click();
  await expect(page.locator('.toast')).toContainText('actualizado a la versión 2026.11.01');
  await expect(page.locator('[data-tool="ytdlp"]')).toContainText('2026.11.01');
});

test('on Android, without access to the storage the notice asks for it and goes away once granted', async ({ page }) => {
  await page.goto('/?mockAndroid&mockNoStorage#/download');
  const notice = page.locator('.status-strip.warning');
  await expect(notice).toContainText('acceso a todos los archivos');
  await notice.getByRole('button', { name: 'Dar acceso' }).click();
  await expect(notice).toBeHidden();
});

test('the interface scale is in Ajustes on a phone and changes the size of everything', async ({ page }) => {
  await page.goto('/?mockAndroid#/settings');
  await page.locator('.settings-tab', { hasText: 'Interfaz' }).click();
  const row = page.locator('.row', { hasText: 'Escala' });
  await expect(row).toBeVisible();
  await row.locator('select').selectOption('150');
  const zoom = await page.evaluate(() => getComputedStyle(document.getElementById('app')!).zoom);
  expect(zoom).toBe('1.5');
  const widths = await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
  expect(widths[0]).toBe(widths[1]);
});

test('a video floats by itself on a phone, in a smaller panel above the bars', async ({ page }) => {
  await page.goto('/#/player?b=pl3:MP4');
  await page.locator('.track-row').first().click();
  await page.locator('.player-bar .player-now').click();
  await expect(page.locator('.np-art')).toBeVisible();
  await expect(page.locator('.np-art .pip-button')).toBeVisible();
  await page.goto('/#/home');
  const panel = page.locator('.pip-panel');
  await expect(panel).toBeVisible();
  const box = await panel.boundingBox();
  expect(box!.width).toBeLessThan(260);
  expect(box!.x + box!.width).toBeLessThanOrEqual(390);
  expect(box!.y + box!.height).toBeLessThan(844 - 50);
});

test('on Android, the keyboard over a field hides the bars and the summary, and they come back with it', async ({ page }) => {
  await page.goto('/?mockAndroid#/download');
  await page.locator('.player-bar').waitFor({ state: 'hidden' });
  const url = page.locator('.fp-body input').first();
  await expect(page.locator('.fp-foot')).toBeVisible();
  await url.focus();
  await page.setViewportSize({ width: 390, height: 380 });
  await expect(page.locator('.fp-foot')).toBeHidden();
  await expect(page.locator('.sidebar')).toBeHidden();
  const box = await url.boundingBox();
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.y + box!.height).toBeLessThanOrEqual(380);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('.fp-foot')).toBeVisible();
  await expect(page.locator('.sidebar')).toBeVisible();
});

test('a search field has an «x» that empties it, also with a finger', async ({ page }) => {
  await page.goto('/#/manager');
  const search = page.getByPlaceholder('Buscar playlist').first();
  const total = await page.locator('.screen').innerText();
  await search.fill('zzzzqx');
  expect(await page.locator('.screen').innerText()).not.toBe(total);
  const box = await search.boundingBox();
  // the clear button sits at the right end of the field
  await page.touchscreen.tap(box!.x + box!.width - 14, box!.y + box!.height / 2);
  await expect(search).toHaveValue('');
  expect(await page.locator('.screen').innerText()).toBe(total);
});
