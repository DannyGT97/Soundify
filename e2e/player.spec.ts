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

test('the bar: duration once opened, next moves on, repeat cycles and previous restarts', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  const titles = await page.locator('.track-row .row-title').allTextContents();
  await page.locator('.track-row').first().click();
  const bar = page.locator('.player-bar');
  const [position, duration] = [bar.locator('.player-time').first(), bar.locator('.player-time').last()];
  // The length comes from the engine once the file is open, not from the archive.
  await page.clock.runFor(100);
  await expect(duration).not.toHaveText('0:00');
  await expect(bar.locator('.player-now .row-title')).toHaveText(titles[0] ?? '');

  await bar.locator('.player-control[title="Siguiente"]').click();
  await expect(bar.locator('.player-now .row-title')).toHaveText(titles[1] ?? '');

  const repeat = bar.locator('.player-control[title^="Repetir"]');
  await expect(repeat).toHaveAttribute('title', 'Repetir: no');
  await repeat.click();
  await expect(repeat).toHaveAttribute('title', 'Repetir: toda la playlist');
  await repeat.click();
  await expect(repeat).toHaveAttribute('title', 'Repetir: este archivo');
  await repeat.click();
  await expect(repeat).toHaveAttribute('title', 'Repetir: no');

  // Past the first seconds "previous" restarts the file instead of going back.
  await page.clock.runFor(5000);
  await expect(position).toHaveText('0:05');
  await bar.locator('.player-control[title="Anterior"]').click();
  await expect(position).toHaveText('0:00');
  await expect(bar.locator('.player-now .row-title')).toHaveText(titles[1] ?? '');
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

test('regression: clicking the file already playing only opens Reproduciendo, it does not start it again', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const position = page.locator('.player-bar .player-time').first();
  await page.clock.runFor(4000);
  await expect(position).toHaveText('0:04');

  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  await expect(page).toHaveURL(/#\/now-playing/);
  await expect(position).toHaveText('0:04');
});

test('regression: space plays and pauses instead of pressing the last button again', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const bar = page.locator('.player-bar');
  const position = bar.locator('.player-time').first();

  // The focus stays on «Siguiente»: space must pause, not skip to another file.
  await bar.locator('.player-control[title="Siguiente"]').click();
  const title = await bar.locator('.player-now .row-title').textContent();
  await page.clock.runFor(2000);
  await expect(position).toHaveText('0:02');
  await page.keyboard.press('Space');
  await page.clock.runFor(3000);
  await expect(position).toHaveText('0:02');
  await expect(bar.locator('.player-now .row-title')).toHaveText(title ?? '');

  await page.keyboard.press('Space');
  await page.clock.runFor(2000);
  await expect(position).toHaveText('0:04');
});

test('with the player only in the side menu, Reproduciendo has the volume', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Interfaz' }).click();
  await page.locator('select').first().selectOption('side');
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  await expect(page.locator('.player-bar')).toBeHidden();
  await expect(page.locator('.np-controls .player-volume .player-slider')).toBeVisible();
});
