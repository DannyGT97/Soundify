import { expect, test } from '@playwright/test';

// Playback through the PlayerBackend (the simulated one in the mock): the bar
// appears, the clock moves and ✕ unloads it (the Java app's PlayerUiTest).

test('playing a file shows the bar and moves the clock; closing hides it', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  const bar = page.locator('.player-bar');
  await expect(bar).toBeHidden();

  await page.locator('.track-row').first().click();
  await expect(page).toHaveURL(/#\/now-playing/);
  await expect(bar).toBeVisible();
  await expect(bar.locator('.player-time').first()).toHaveText('0:00');

  await page.clock.runFor(3000);
  await expect(bar.locator('.player-time').first()).toHaveText('0:03');

  await bar.locator('.player-play').click();
  await page.clock.runFor(3000);
  await expect(bar.locator('.player-time').first()).toHaveText('0:03');

  await bar.locator('.player-control[title="Cerrar el reproductor"]').click();
  await expect(bar).toBeHidden();
});

test('a download from the URL screen shows in the menu indicator', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/download');
  await page.locator('input.input').first().fill('https://www.youtube.com/watch?v=abc');
  await page.getByRole('button', { name: 'Descargar' }).click();
  await expect(page.locator('.tasks-pill')).toBeVisible();
  await page.clock.runFor(10_000);
  await expect(page.locator('.tasks-pill-title')).toHaveText('Terminada');
});
