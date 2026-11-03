import { expect, test } from '@playwright/test';

// Ajustes against the mock, whose preferences live in sessionStorage the way
// the real ones live in settings.json: a reload keeps them.

test('switching the language redraws the whole app and is remembered', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Interfaz' }).click();
  await page.locator('[data-key="settings.interface.language"] select').selectOption('en');

  await expect(page.locator('.nav-label', { hasText: 'Playlist Manager' })).toBeVisible();
  await expect(page.locator('.settings-tab.active')).toHaveText('Interface');
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');

  await page.reload();
  await expect(page.locator('.nav-label', { hasText: 'Playlist Manager' })).toBeVisible();
});

test('the search finds by title first, without accents, and takes you to the setting', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-search').fill('IDIOMA');
  await expect(page.locator('.search-hit .section-title').first()).toHaveText('Idioma');
  await expect(page.locator('.settings-tab.active')).toHaveCount(0);

  await page.locator('.search-hit').first().click();
  await expect(page.locator('.settings-tab.active')).toHaveText('Interfaz');
  await expect(page.locator('[data-key="settings.interface.language"]')).toHaveClass(/highlight/);
});

test('the theme is applied and survives a reload', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Interfaz' }).click();
  await page.locator('[data-key="settings.interface.theme"] button').nth(1).click();
  const chosen = await page.locator('html').getAttribute('data-theme');
  expect(chosen).not.toBe('dark');

  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', chosen ?? '');
});

test('cookies say their state, and a reset brings the factory values back on the spot', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Avanzado' }).click();
  const status = page.locator('[data-key="settings.advanced.cookies"] .help');
  await expect(status).toContainText('Sin cookies configuradas');

  await page.getByRole('button', { name: 'Seleccionar' }).click();
  await expect(status).toHaveClass(/status-text-ok/);

  await page.getByRole('button', { name: 'Reset', exact: true }).click();
  await page.locator('#overlay-root .danger-button').click();
  await expect(page.locator('.toast-text')).toContainText('Configuración restablecida');
  await expect(status).toContainText('Sin cookies configuradas');
  await expect(page.locator('[data-key="settings.advanced.cookies"] input')).toHaveValue('');
});

test('checking for updates opens the window with the notes, and a failed install is said', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Actualizaciones' }).click();
  await page.getByRole('button', { name: 'Buscar actualizaciones' }).click();

  await expect(page.getByText('Hay una versión nueva: v9.9.9')).toBeVisible();
  await expect(page.locator('.update-notes')).toContainText('texto plano');
  await page.getByRole('button', { name: 'Ahora no' }).click();
  await expect(page.locator('.update-notes')).toHaveCount(0);

  // Found, so the panel's button now offers it; the mock is a development build.
  await page.getByRole('button', { name: 'Descargar e instalar v9.9.9' }).click();
  await expect(page.locator('.toast-error')).toContainText('No se ha instalado nada');
  await expect(page.getByRole('button', { name: 'Buscar ahora' })).toBeEnabled();
});
