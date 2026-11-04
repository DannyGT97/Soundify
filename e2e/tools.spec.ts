import { expect, test, type Page } from '@playwright/test';

// The tools against the mock: `?mockTools=missing` starts with none of the
// three installed, `?mockTools=failing` the same with Deno failing once.

test('the assistant opens on a first start and installs the three tools', async ({ page }) => {
  await page.goto('/?mockTools=missing#/home');
  const dialog = page.locator('.overlay-window');
  await expect(dialog.locator('.overlay-title')).toHaveText('Antes de descargar');
  await expect(dialog).toContainText('Se descargarán 3 (≈ 178 MB)');

  await dialog.getByRole('button', { name: 'Instalar lo necesario' }).click();
  await expect(dialog.locator('.overlay-title')).toHaveText('Instalando…');
  await expect(dialog.locator('.overlay-title')).toHaveText('Todo listo', { timeout: 15_000 });
  await expect(dialog.locator('.tools-chip-ok')).toHaveCount(3);

  await dialog.getByRole('button', { name: 'Empezar a usar Soundify' }).click();
  await page.goto('/?mockTools=missing#/download');
  // Same page, only the route changes: what was just installed stays installed.
  await expect(page.locator('.status-strip.warning')).toBeHidden();
  await page.locator('input.input').first().fill('https://www.youtube.com/watch?v=abc');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  await expect(page.getByRole('button', { name: 'Descargar', exact: true })).toBeEnabled();
});

test('«Ahora no» leaves Descarga blocked and points to Ajustes → Herramientas', async ({ page }) => {
  await page.goto('/?mockTools=missing#/download');
  await page.locator('.overlay-window').getByRole('button', { name: 'Ahora no' }).click();

  await expect(page.locator('.status-strip.warning')).toContainText('yt-dlp, ffmpeg');
  await expect(page.getByRole('button', { name: 'Descargar', exact: true })).toBeDisabled();

  await page.locator('.toast').getByRole('button', { name: 'Ir a Ajustes → Herramientas' }).click();
  await expect(page.locator('.settings-tab.active')).toContainText('Herramientas');
  await expect(page.locator('.settings-tab.active .tab-badge')).toHaveText('3');

  await page.locator('[data-tool="ytdlp"]').getByRole('button', { name: 'Instalar' }).click();
  await expect(page.locator('[data-tool="ytdlp"]')).toContainText('✓ Instalado', { timeout: 10_000 });
  await expect(page.locator('.settings-tab.active .tab-badge')).toHaveText('2');
});

test('a failed download can be retried from the assistant', async ({ page }) => {
  await page.goto('/?mockTools=failing#/home');
  const dialog = page.locator('.overlay-window');
  await dialog.getByRole('button', { name: 'Instalar lo necesario' }).click();
  await expect(dialog.locator('.overlay-title')).toHaveText('No se pudo completar', { timeout: 15_000 });
  await expect(dialog.locator('.tools-chip-error')).toHaveCount(1);

  await dialog.getByRole('button', { name: 'Reintentar' }).click();
  await expect(dialog.locator('.overlay-title')).toHaveText('Todo listo', { timeout: 15_000 });
});

test('with the tools in place nothing asks for them', async ({ page }) => {
  await page.goto('/#/download');
  await expect(page.locator('.status-strip.warning')).toBeHidden();
  await expect(page.locator('.overlay-window')).toHaveCount(0);
});

test.describe('without yt-dlp and ffmpeg nothing starts', () => {
  const dismiss = (page: Page) => page.locator('.overlay-window').getByRole('button', { name: 'Ahora no' }).click();

  test('Descarga por URL: the notice is there and Intro does not start anything', async ({ page }) => {
    await page.goto('/?mockTools=missing#/download');
    await dismiss(page);
    await expect(page.locator('.status-strip.warning')).toContainText('yt-dlp, ffmpeg');
    await page.locator('input.input').first().fill('https://www.youtube.com/watch?v=abc');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('button', { name: 'Descargar', exact: true })).toBeDisabled();
    await expect(page.locator('.tasks-pill')).toBeHidden();
  });

  test('Crear Playlist: the notice is there and both buttons wait', async ({ page }) => {
    await page.goto('/?mockTools=missing#/new-playlist');
    await dismiss(page);
    await expect(page.locator('.status-strip.warning')).toContainText('yt-dlp, ffmpeg');
    await expect(page.getByRole('button', { name: 'Crear y descargar' })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Crear playlist', exact: true })).toBeDisabled();
  });

  test('Gestor: the notice is there and Sincronizar waits, until they are installed', async ({ page }) => {
    await page.goto('/?mockTools=missing#/manager?open=pl1');
    await dismiss(page);
    await expect(page.locator('.status-strip.warning')).toContainText('yt-dlp, ffmpeg');
    const sync = () => page.locator('.pm-block').first().getByRole('button', { name: 'Sincronizar' });
    await expect(sync()).toBeDisabled();

    await page.locator('.status-strip.warning').getByRole('button', { name: 'Instalar ahora' }).click();
    const dialog = page.locator('.overlay-window');
    await dialog.getByRole('button', { name: 'Instalar lo necesario' }).click();
    await dialog.getByRole('button', { name: 'Empezar a usar Soundify' }).click({ timeout: 15_000 });
    await expect(page.locator('.status-strip.warning')).toBeHidden();
    await expect(sync()).toBeEnabled();
  });
});

test('Ajustes → Actualizaciones has the two independent switches', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Actualizaciones' }).click();
  await expect(page.getByText('Actualizar yt-dlp, ffmpeg y Deno', { exact: false })).toBeVisible();
  await page.locator('.settings-tab', { hasText: 'Herramientas' }).click();
  await expect(page.getByText('Actualizar yt-dlp, ffmpeg y Deno', { exact: false })).toHaveCount(0);
});
