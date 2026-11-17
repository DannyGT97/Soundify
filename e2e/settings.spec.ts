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
  // The explorer lists only .txt files; the one picked goes into the box.
  const explorer = page.locator('.mtp.xp');
  await explorer.locator('.xp-place', { hasText: 'Descargas' }).click();
  await explorer.locator('.xp-row', { hasText: 'cookies.txt' }).click();
  await explorer.getByRole('button', { name: 'Usar este archivo' }).click();
  await expect(page.locator('[data-key="settings.advanced.cookies"] input')).toHaveValue('C:\\Users\\Ana\\Downloads\\cookies.txt');
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

test('«Descargar más rápido» is one experimental switch that sets the three speed-ups, is off by default, remembered and reset', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Avanzado' }).click();
  const row = page.locator('[data-key="settings.advanced.faster"]');
  const box = row.locator('input.switch');
  await expect(row).toContainText('Descargar más rápido (experimental)');
  await expect(row).toContainText('YouTube te bloquee');
  await expect(box).not.toBeChecked();
  // The three old switches are gone: this is the only one.
  await expect(page.locator('[data-key="settings.advanced.parallel"]')).toHaveCount(0);
  await expect(page.locator('[data-key="settings.advanced.fragments"]')).toHaveCount(0);

  await box.check();
  await page.reload();
  await page.locator('.settings-tab', { hasText: 'Avanzado' }).click();
  await expect(box).toBeChecked();

  await page.getByRole('button', { name: 'Reset', exact: true }).click();
  await page.locator('#overlay-root .danger-button').click();
  await expect(page.locator('.toast-text')).toContainText('Configuración restablecida');
  await expect(box).not.toBeChecked();
});

test('Ctrl + / Ctrl − / Ctrl 0 and Ctrl + wheel change the scale, and Ajustes follows', async ({ page }) => {
  await page.goto('/#/settings');
  const zoom = () => page.evaluate(() => document.getElementById('app')?.style.zoom);
  await page.keyboard.press('Control+=');
  await expect.poll(zoom).toBe('1.1');
  await page.keyboard.press('Control++');
  await expect.poll(zoom).toBe('1.25');
  await page.keyboard.press('Control+-');
  await expect.poll(zoom).toBe('1.1');
  await page.keyboard.press('Control+0');
  await expect.poll(zoom).toBe('1');
  await page.mouse.move(400, 300);
  await page.keyboard.down('Control');
  await page.mouse.wheel(0, -100);
  await page.keyboard.up('Control');
  await expect.poll(zoom).toBe('1.1');
});

test('with a big scale the side menu scrolls by itself, and the page does not move with it', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 600 });
  await page.goto('/#/settings');
  for (let i = 0; i < 4; i++) await page.keyboard.press('Control+=');
  const info = await page.evaluate(() => {
    const side = document.querySelector('.sidebar') as HTMLElement;
    const screen = document.querySelector('.screen') as HTMLElement;
    return { scrolls: side.scrollHeight > side.clientHeight, overflowY: getComputedStyle(side).overflowY, pageBefore: screen.scrollTop };
  });
  expect(info.overflowY).toBe('auto');
  expect(info.scrolls).toBe(true);
  await page.evaluate(() => { (document.querySelector('.sidebar') as HTMLElement).scrollTop = 9999; });
  const after = await page.evaluate(() => ({ side: (document.querySelector('.sidebar') as HTMLElement).scrollTop, page: (document.querySelector('.screen') as HTMLElement).scrollTop }));
  expect(after.side).toBeGreaterThan(0);
  expect(after.page).toBe(info.pageBefore);
  // The last entry (Ajustes) can be reached.
  await expect(page.locator('.sidebar-version')).toBeVisible();
});
