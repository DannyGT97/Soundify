import { expect, test, type Page } from '@playwright/test';

// Soundify Connect with the simulated network of `api/mock/connect.ts`. Like the real thing,
// devices are open by default: «Salón» is already connected and nothing is typed anywhere.
// «Tablet de la cocina» is strict and needs the PIN 123456.

async function openDevices(page: Page, route = '/?mockConnect#/settings'): Promise<void> {
  await page.goto(route);
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await expect(page.locator('.overlay-window')).toBeVisible();
}

const row = (page: Page, id: string) => page.locator(`[data-device="${id}"]`);
const barTitle = (page: Page) => page.locator('.player-bar .player-now .row-title');

test('an open device just shows up connected: nothing to pair, nothing to type', async ({ page }) => {
  await openDevices(page);
  const salon = row(page, 'tv');
  await expect(salon).toContainText('Conectado');
  await expect(salon.getByRole('button', { name: 'Controlar' })).toBeVisible();
  await expect(salon.locator('.cx-pin-input')).toHaveCount(0);
  // This device shows no PIN either while it is open.
  await expect(page.locator('.cx-pin-code')).toHaveCount(0);
});

test('a strict device asks for its PIN: a wrong one says so, the right one connects', async ({ page }) => {
  await openDevices(page);
  const tablet = row(page, 'tablet');
  await expect(tablet).toContainText('Pide un PIN');

  await tablet.locator('.cx-pin-input').fill('000000');
  await tablet.getByRole('button', { name: 'Emparejar' }).click();
  await expect(page.locator('.toast', { hasText: 'PIN incorrecto' })).toBeVisible();

  await tablet.locator('.cx-pin-input').fill('123 456');
  await tablet.getByRole('button', { name: 'Emparejar' }).click();
  await expect(tablet).toContainText('Conectado');
  await expect(tablet.getByRole('button', { name: 'Controlar' })).toBeVisible();
});

test('a PIN that is not six digits is not even sent', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tablet').locator('.cx-pin-input').fill('12');
  await row(page, 'tablet').getByRole('button', { name: 'Emparejar' }).click();
  await expect(page.locator('.toast', { hasText: '6 cifras' })).toBeVisible();
  await expect(row(page, 'tablet')).toContainText('Pide un PIN');
});

test('asking for a PIN is a switch: this device then shows its own PIN', async ({ page }) => {
  await openDevices(page);
  await expect(page.locator('.cx-pin-code')).toHaveCount(0);
  await page.locator('.cx-switch', { hasText: 'Pedir un PIN' }).locator('.switch').check();
  await expect(page.locator('.cx-pin-code')).toHaveText('482 915');
  await page.locator('.cx-switch', { hasText: 'Pedir un PIN' }).locator('.switch').uncheck();
  await expect(page.locator('.cx-pin-code')).toHaveCount(0);
});

test('turning Soundify Connect off hides the devices', async ({ page }) => {
  await openDevices(page);
  await page.locator('.cx-switch', { hasText: 'Soundify Connect' }).locator('.switch').uncheck();
  await expect(page.locator('[data-device]')).toHaveCount(0);
});

test('one tap controls a connected device: the bar shows its music and the buttons drive it', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
  await page.locator('.player-bar .player-control[title="Siguiente"]').click();
  await expect(barTitle(page)).toHaveText('Siguiente del salón');

  // Reproduciendo says where it sounds.
  await page.goto('/?mockConnect#/now-playing');
  await expect(page.locator('.np-device')).toContainText('Salón');
});

test('a device that starts playing is noticed, and one tap on the notice controls it', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  await page.evaluate(() => (window as unknown as { __connect: { salonPlays: (on: boolean) => void } }).__connect.salonPlays(true));
  const notice = page.locator('.toast', { hasText: 'Salón' });
  await expect(notice).toContainText('Canción del salón');
  await notice.getByRole('button', { name: 'Controlar' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
  // The devices button of the bar lights up while it is controlled.
  await expect(page.locator('.player-bar .devices-button')).toHaveClass(/on/);
});

test('the list says what each device is playing', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  await page.evaluate(() => (window as unknown as { __connect: { salonPlays: (on: boolean) => void } }).__connect.salonPlays(true));
  await page.locator('.sidebar .devices-button').click();
  await expect(row(page, 'tv')).toContainText('Sonando: Canción del salón');
});

test('connection lost: this device is the one controlled again and the user is told', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const own = await barTitle(page).textContent();
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');

  await page.evaluate(() => (window as unknown as { __connect: { drop: () => void } }).__connect.drop());
  await expect(page.locator('.toast', { hasText: 'Se perdió la conexión' })).toBeVisible();
  await expect(barTitle(page)).toHaveText(own ?? '');
});

test('being controlled: a command from another device moves this player and its state is published', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const first = await barTitle(page).textContent();

  const incoming = (command: object) => page.evaluate(c => (window as unknown as { __connect: { incoming: (c: object) => void } }).__connect.incoming(c), command);
  await incoming({ cmd: 'next' });
  await expect(barTitle(page)).not.toHaveText(first ?? '');

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
  const title = await barTitle(page).textContent();
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Pasar mi música' }).click();
  await expect(barTitle(page)).toHaveText(title ?? '');
  await expect(barTitle(page)).not.toHaveText('Canción del salón');
});

test('what is typed in the address and PIN boxes survives the list redrawing itself', async ({ page }) => {
  await openDevices(page);
  const address = page.locator('.cx-add input');
  await address.click();
  await address.pressSequentially('192.168');
  const pin = row(page, 'tablet').locator('.cx-pin-input');
  await pin.fill('12');
  await address.click();
  // The window reloads its list every few seconds.
  await page.waitForTimeout(3600);
  await expect(address).toHaveValue('192.168');
  await expect(address).toBeFocused();
  await expect(pin).toHaveValue('12');
  await address.pressSequentially('.1.31');
  await expect(address).toHaveValue('192.168.1.31');
});

test('Windows: a Public network is warned about and the firewall rule is one button', async ({ page }) => {
  await openDevices(page, '/?mockConnect&mockFirewall#/settings');
  await expect(page.locator('.cx-warning', { hasText: 'Pública' })).toBeVisible();
  // One click makes the network private; the rule is another one.
  await page.getByRole('button', { name: 'Cambiar a red privada' }).click();
  await expect(page.locator('.toast', { hasText: 'tu red ahora es privada' })).toBeVisible();
  await expect(page.locator('.cx-warning', { hasText: 'Pública' })).toHaveCount(0);
  const card = page.locator('.cx-firewall');
  await expect(card).toContainText('Permitir Soundify en el cortafuegos');
  await card.getByRole('button', { name: 'Permitir en el cortafuegos' }).click();
  await expect(page.locator('.toast', { hasText: 'Soundify puede recibir conexiones' })).toBeVisible();
  await expect(page.locator('.cx-firewall')).toHaveCount(0);
});

test('without Windows there is nothing about the firewall', async ({ page }) => {
  await openDevices(page);
  await expect(page.locator('.cx-firewall')).toHaveCount(0);
});
