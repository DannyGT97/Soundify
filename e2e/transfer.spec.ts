import { expect, test } from '@playwright/test';

// "Copiar playlist" and "Importar" against the mock: the steps follow one
// another and each ends in its notice (the texts are tested in transfer.test.ts).

test('copying a playlist asks to confirm, shows progress and ends in a notice', async ({ page }) => {
  await page.goto('/#/manager?open=pl1');
  await page.locator('.pm-copy-button').first().click();
  await page.getByRole('button', { name: 'Copiar a una carpeta local…' }).first().click();

  const dialog = page.getByRole('dialog');
  await expect(dialog.getByRole('checkbox')).toBeChecked();
  await dialog.getByRole('button', { name: 'Copiar', exact: true }).click();

  await expect(page.locator('.toast-success')).toContainText('Registrada en el Gestor.');
  await expect(page.getByRole('dialog')).toBeHidden();
});

test('cancelling a copy undoes it and says so', async ({ page }) => {
  await page.goto('/#/manager?open=pl1');
  await page.locator('.pm-copy-button').first().click();
  await page.getByRole('button', { name: 'Copiar a una carpeta local…' }).first().click();
  await page.getByRole('dialog').getByRole('button', { name: 'Copiar', exact: true }).click();
  // The confirmation is still fading out (with its own "Cancelar") when the progress window opens.
  await page.locator('.overlay-scrim:not(.overlay-leaving)').getByRole('button', { name: 'Cancelar' }).click();

  await expect(page.locator('.toast-error')).toContainText('Copia cancelada');
});

test('importing from the help window ends in a notice', async ({ page }) => {
  await page.goto('/#/manager');
  await page.getByRole('button', { name: 'Importar' }).click();
  await page.getByRole('button', { name: 'Explorar archivos' }).click();

  await expect(page.locator('.toast-success')).toContainText('importada');
});

test('copying to a phone goes through the picker and names where it went', async ({ page }) => {
  await page.goto('/#/manager?open=pl1');
  await page.locator('.pm-copy-button').first().click();
  await page.getByRole('button', { name: 'Copiar a un dispositivo…' }).first().click();

  const picker = page.getByRole('dialog');
  await picker.getByRole('button', { name: 'Music' }).click();
  await expect(picker.getByRole('button', { name: 'Soundify' })).toBeVisible();
  await picker.getByRole('button', { name: 'Seleccionar esta carpeta' }).click();

  await expect(page.locator('.toast-success')).toContainText('Pixel 8 / Music');
});

test('importing from a phone scans it and ends in a notice', async ({ page }) => {
  await page.goto('/#/manager');
  await page.getByRole('button', { name: 'Importar' }).click();
  await page.getByRole('button', { name: 'Buscar en dispositivo' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Seleccionar esta carpeta' }).click();

  await expect(page.locator('.toast-success')).toContainText('importada');
});

test('«Eliminar todo» says what it took and the format leaves the playlist', async ({ page }) => {
  await page.goto('/#/manager?open=pl1');
  await page.locator('.pm-delete').first().click();
  // The body names the folder that is about to be emptied.
  await expect(page.getByRole('dialog')).toContainText('Música');
  await page.getByRole('button', { name: 'Eliminar todo (archivos + registro)' }).click();

  await expect(page.locator('.toast-success')).toContainText('Eliminado "Chill de domingo"');
  // Its FLAC block stays: only the MP3 one was deleted.
  await expect(page.locator('.pm-block')).toHaveCount(1);
  await expect(page.locator('.pm-block .chip')).toHaveText('FLAC');
});
