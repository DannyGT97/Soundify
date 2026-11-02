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
