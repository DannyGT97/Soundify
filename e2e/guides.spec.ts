import { expect, test } from '@playwright/test';

// The first-use guides: each one comes once, with its steps, and «Restablecer avisos y guías» brings them back. The mock
// starts with every guide seen (so the other tests do not meet one by surprise): `?mockGuides` starts with none.

// The window that is closing is still there for a moment (it fades): only the one that stays counts.
const dialog = (page: import('@playwright/test').Page) => page.locator('.overlay-scrim:not(.overlay-leaving) .overlay-window');

test('the first start walks through the welcome in three steps and never comes back by itself', async ({ page }) => {
  await page.goto('/?mockGuides#/home');
  await expect(dialog(page)).toContainText('Descarga tu música');
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await expect(dialog(page)).toContainText('Escúchala donde quieras');
  await dialog(page).getByRole('button', { name: 'Atrás' }).click();
  await expect(dialog(page)).toContainText('Descarga tu música');
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await expect(dialog(page)).toContainText('Úsala con tus otros dispositivos');
  await dialog(page).getByRole('button', { name: 'Empezar' }).click();
  await expect(dialog(page)).toHaveCount(0);
  // Seen: it does not come back when the page starts again.
  await page.reload();
  await expect(page.locator('.screen')).toBeVisible();
  await expect(dialog(page)).toHaveCount(0);
});

test('on Android the welcome is followed by what the permissions are for', async ({ page }) => {
  await page.goto('/?mockGuides&mockAndroid#/home');
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await dialog(page).getByRole('button', { name: 'Empezar' }).click();
  await expect(dialog(page)).toContainText('Acceso a todos los archivos');
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await expect(dialog(page)).toContainText('Red local');
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await dialog(page).getByRole('button', { name: 'Entendido' }).click();
  await expect(dialog(page)).toHaveCount(0);
});

test('closing a guide with the cross marks it as seen and does not go ahead with what it stood in front of', async ({ page }) => {
  await page.goto('/?mockGuides=connect&mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await expect(dialog(page)).toContainText('Tus dispositivos, juntos');
  await dialog(page).getByRole('button', { name: 'Cerrar' }).click();
  await expect(dialog(page)).toHaveCount(0);
  // Seen: now the window itself opens.
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await expect(dialog(page)).toContainText('Soundify Connect');
  await expect(dialog(page)).not.toContainText('Tus dispositivos, juntos');
});

test('the guide of the devices window ends with the window itself', async ({ page }) => {
  await page.goto('/?mockGuides=connect&mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await dialog(page).getByRole('button', { name: 'Siguiente' }).click();
  await dialog(page).getByRole('button', { name: 'Entendido' }).click();
  await expect(dialog(page)).toContainText('Los dispositivos con Soundify de tu red');
});

test('«Escuchar aquí» is explained the first time, and goes ahead when accepted', async ({ page }) => {
  await page.goto('/?mockGuides=listen&mockConnect#/player');
  await page.evaluate(() => (window as unknown as { __connect: { salonPlays: (p: boolean) => void } }).__connect.salonPlays(true));
  await expect(page.locator('.devcard[data-device="tv"]')).toContainText('Sonando');
  await page.locator('[data-listen="tv"]').click();
  await expect(dialog(page)).toContainText('Vas a escuchar lo de «Salón»');
  await dialog(page).getByRole('button', { name: 'Escuchar aquí' }).click();
  await expect(page.locator('.eyebrow.cast-eyebrow')).toContainText('Escuchando desde «Salón»');
});

test('the first pass to another device says what will happen before it happens', async ({ page }) => {
  await page.goto('/?mockGuides=pass&mockConnect#/player?b=pl1:MP3');
  await page.locator('.tracks .track-row:not(.off)').first().click();
  await page.evaluate(() => { location.hash = '#/player'; });
  await page.locator('.devices-button:visible').first().click();
  await page.locator('[data-pass="tv"]').click();
  await expect(dialog(page)).toContainText('Lo que suena aquí sonará en «Salón»');
  await dialog(page).getByRole('button', { name: 'Pasar la música' }).click();
  await expect(page.locator('.devices-button:visible').first()).toHaveClass(/is-cast/);
});

test('the first remote playlist says it is the library of another device', async ({ page }) => {
  await page.goto('/?mockGuides=remote&mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await page.locator('[data-device="tv"]').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/remote-playlist'; });
  await expect(dialog(page)).toContainText('Estás viendo la biblioteca de «Salón»');
  await dialog(page).getByRole('button', { name: 'Entendido' }).click();
  await expect(dialog(page)).toHaveCount(0);
});

test('turning the PIN off asks first and says anybody could join', async ({ page }) => {
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await page.locator('.cx-switch', { hasText: 'Pedir un PIN' }).locator('.switch').check();
  await page.locator('.cx-switch', { hasText: 'Pedir un PIN' }).locator('.switch').click();
  await expect(dialog(page)).toContainText('Cualquier dispositivo de esta red podrá emparejarse sin PIN');
});

test('«Restablecer avisos y guías» brings the guides back', async ({ page }) => {
  await page.goto('/#/settings');
  const reset = page.getByRole('button', { name: 'Restablecer avisos y guías' });
  // All the guides are seen in the mock: the button has something to do.
  await expect(reset).toBeEnabled();
  await reset.click();
  await expect(page.getByText('Todas las ventanas se muestran')).toBeVisible();
  // Nothing hidden now: the button is still there, and says so instead of doing nothing.
  await expect(reset).toBeEnabled();
  await reset.click();
  await expect(page.locator('.toast', { hasText: 'Todas las ventanas se muestran' })).toBeVisible();
  await page.reload();
  await expect(dialog(page)).toContainText('Descarga tu música');
});
