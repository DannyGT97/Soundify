import { expect, test } from '@playwright/test';

// «Crear Playlist» against the mock: a playlist already registered in that folder and format is
// not reused in silence; the user chooses (the back refuses it with `newPlaylist.alreadyExists`).

async function create(page: import('@playwright/test').Page) {
  await page.getByRole('button', { name: 'Crear playlist', exact: true }).click();
}

test('creating the same playlist again in the same folder asks before reusing it', async ({ page }) => {
  await page.goto('/#/new-playlist');
  await page.locator('#url').fill('https://www.youtube.com/playlist?list=PL1');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();

  await create(page);
  await expect(page).toHaveURL(/#\/manager/);

  await page.goto('/#/new-playlist');
  await page.locator('#url').fill('https://www.youtube.com/playlist?list=PL1');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  await create(page);

  const dialog = page.locator('.overlay-window');
  await expect(dialog.locator('.overlay-title')).toHaveText('Playlist ya existente');
  await expect(dialog).toContainText('ya está en esa carpeta');
  // Still on the same screen: nothing was registered.
  await expect(page).toHaveURL(/#\/new-playlist/);

  await dialog.getByRole('button', { name: 'Continuar igualmente' }).click();
  await expect(page).toHaveURL(/#\/manager/);
});
