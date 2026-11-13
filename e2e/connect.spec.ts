import { expect, test, type Page } from '@playwright/test';

// Soundify Connect with the simulated network of `api/mock/connect.ts` (`?mockConnect` turns it on):
// pairing with a PIN, controlling another device from the player, and being controlled.

const row = (page: Page, id: string) => page.locator(`[data-device="${id}"]`);

test('a new device is paired with its PIN: a wrong one says so, the right one pairs', async ({ page }) => {
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  const salon = row(page, 'tv');
  await expect(salon).toContainText('Nuevo en la red');

  await salon.locator('.cx-pin-input').fill('000000');
  await salon.getByRole('button', { name: 'Emparejar' }).click();
  await expect(page.locator('.toast', { hasText: 'PIN incorrecto' })).toBeVisible();

  await salon.locator('.cx-pin-input').fill('123 456');
  await salon.getByRole('button', { name: 'Emparejar' }).click();
  await expect(salon).toContainText('Emparejado');
  await expect(salon.getByRole('button', { name: 'Controlar' })).toBeVisible();
  // The other one is still new.
  await expect(row(page, 'tablet')).toContainText('Nuevo en la red');
});

test('a PIN that is not six digits is not even sent', async ({ page }) => {
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await row(page, 'tv').locator('.cx-pin-input').fill('12');
  await row(page, 'tv').getByRole('button', { name: 'Emparejar' }).click();
  await expect(page.locator('.toast', { hasText: '6 cifras' })).toBeVisible();
  await expect(row(page, 'tv')).toContainText('Nuevo en la red');
});

test('the device shows its own PIN to be paired with, and it can be turned off', async ({ page }) => {
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await expect(page.locator('.cx-pin-code')).toHaveText('482 915');
  await page.locator('.cx-switch .switch').uncheck();
  await expect(page.locator('.cx-pin-code')).toHaveCount(0);
  await expect(page.locator('[data-device]')).toHaveCount(0);
});

test('controlling a paired device: the bar shows its music and the buttons drive it', async ({ page }) => {
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  const salon = row(page, 'tv');
  await salon.locator('.cx-pin-input').fill('123456');
  await salon.getByRole('button', { name: 'Emparejar' }).click();
  await salon.getByRole('button', { name: 'Controlar' }).click();

  const bar = page.locator('.player-bar');
  await expect(bar.locator('.player-now .row-title')).toHaveText('Canción del salón');
  await bar.locator('.player-control[title="Siguiente"]').click();
  await expect(bar.locator('.player-now .row-title')).toHaveText('Siguiente del salón');

  // Reproduciendo says where it sounds, and the chip opens the list to go back.
  await page.goto('/?mockConnect#/now-playing');
  await expect(page.locator('.np-device')).toContainText('Salón');
});

test('connection lost: this device is the one controlled again and the user is told', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const own = await page.locator('.player-bar .player-now .row-title').textContent();
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  const salon = row(page, 'tv');
  await salon.locator('.cx-pin-input').fill('123456');
  await salon.getByRole('button', { name: 'Emparejar' }).click();
  await salon.getByRole('button', { name: 'Controlar' }).click();
  await expect(page.locator('.player-bar .player-now .row-title')).toHaveText('Canción del salón');

  await page.evaluate(() => (window as unknown as { __connect: { drop: () => void } }).__connect.drop());
  await expect(page.locator('.toast', { hasText: 'Se perdió la conexión' })).toBeVisible();
  await expect(page.locator('.player-bar .player-now .row-title')).toHaveText(own ?? '');
});

test('being controlled: a command from another device moves this player and its state is published', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const bar = page.locator('.player-bar .player-now .row-title');
  const first = await bar.textContent();

  const incoming = (command: object) => page.evaluate(c => (window as unknown as { __connect: { incoming: (c: object) => void } }).__connect.incoming(c), command);
  await incoming({ cmd: 'next' });
  await expect(bar).not.toHaveText(first ?? '');

  await incoming({ cmd: 'pause' });
  await expect(page.locator('.player-bar .player-play')).toBeVisible();
  const published = await page.evaluate(() => (window as unknown as { __connect: { published: Array<{ playing: boolean; nowPlaying: { title: string } | null }> } }).__connect.published);
  expect(published.length).toBeGreaterThan(0);
  expect(published.at(-1)?.playing).toBe(false);
  expect(published.at(-1)?.nowPlaying?.title).toBeTruthy();
});

test('handing the music over takes it to the other device from where it was', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const title = await page.locator('.player-bar .player-now .row-title').textContent();
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  const salon = row(page, 'tv');
  await salon.locator('.cx-pin-input').fill('123456');
  await salon.getByRole('button', { name: 'Emparejar' }).click();
  await salon.getByRole('button', { name: 'Pasar mi música' }).click();
  await expect(page.locator('.player-bar .player-now .row-title')).toHaveText(title ?? '');
  await expect(page.locator('.player-bar .player-now .row-title')).not.toHaveText('Canción del salón');
});
