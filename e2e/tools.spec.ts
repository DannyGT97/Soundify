import { expect, test } from '@playwright/test';

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
