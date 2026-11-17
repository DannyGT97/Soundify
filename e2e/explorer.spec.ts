import { expect, test } from '@playwright/test';
import { explorer } from './helpers';

// The explorer against the mock: a made-up computer (src/api/mock/browse.ts) and its Pixel 8.

async function openDestination(page: import('@playwright/test').Page) {
  await page.goto('/#/download');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  await expect(explorer(page)).toBeVisible();
}

test('it opens on Música, names where it will save and takes that folder', async ({ page }) => {
  await openDestination(page);
  const window = explorer(page);
  await expect(window.locator('.crumbs')).toContainText('Music');
  await expect(window.locator('.mtp-foot')).toContainText('C:\\Users\\Ana\\Music');
  // A disk says how much space it has left.
  await expect(window.locator('.xp-place', { hasText: 'Disco (D:)' })).toContainText('libres');

  await window.getByRole('button', { name: 'Guardar aquí' }).click();
  await expect(window).toBeHidden();
  await expect(page.locator('.dest.on small')).toHaveText('C:\\Users\\Ana\\Music');
});

test('a playlist folder is told apart, and choosing inside it warns', async ({ page }) => {
  await openDestination(page);
  const window = explorer(page);
  const playlist = window.locator('.xp-row', { hasText: 'Rock de los 80' });
  await expect(playlist).toContainText('Playlist · MP3');

  await playlist.click();
  await expect(window.locator('.banner.warn-b')).toContainText('ya es una playlist');
});

test('it moves between disks, goes up and filters without accents', async ({ page }) => {
  await openDestination(page);
  const window = explorer(page);
  await window.locator('.xp-place', { hasText: 'Disco (D:)' }).click();
  await expect(window.locator('.xp-row', { hasText: 'Música' })).toBeVisible();
  await window.locator('.xp-row', { hasText: 'Música' }).click();
  await window.locator('.xp-row', { hasText: 'Soundify' }).click();
  await expect(window.locator('.mtp-foot')).toContainText('D:\\Música\\Soundify');

  await window.getByRole('searchbox', { name: 'Filtrar aquí' }).fill('lofi');
  await expect(window.locator('.xp-row')).toHaveCount(0);
  await window.getByRole('searchbox', { name: 'Filtrar aquí' }).fill('estudiar');
  await expect(window.locator('.xp-row')).toHaveCount(1);

  await window.getByRole('button', { name: 'Subir' }).click();
  await expect(window.locator('.crumbs button').last()).toHaveText('Música');
});

test('a folder the system will not show says so, and offers the way back', async ({ page }) => {
  await openDestination(page);
  const window = explorer(page);
  await window.locator('.xp-place', { hasText: 'Disco (C:)' }).click();
  await window.locator('.xp-row', { hasText: 'Windows' }).click();
  await expect(window.locator('.empty')).toContainText('Carpeta bloqueada');
  await expect(window.getByRole('button', { name: 'Guardar aquí' })).toBeDisabled();

  await window.locator('.empty').getByRole('button', { name: 'Subir' }).click();
  await expect(window.locator('.xp-row', { hasText: 'Users' })).toBeVisible();
});

test('«Nueva carpeta» creates it, goes inside it and refuses a name already taken', async ({ page }) => {
  await openDestination(page);
  const window = explorer(page);
  await window.getByRole('button', { name: 'Nueva carpeta' }).click();
  await window.getByLabel('Nueva carpeta').last().fill('Rock de los 80 (mp3)');
  await window.getByLabel('Nueva carpeta').last().press('Enter');
  await expect(page.locator('.toast-error')).toContainText('Ya hay una carpeta');

  await window.getByLabel('Nueva carpeta').last().fill('Mis cosas');
  await window.getByLabel('Nueva carpeta').last().press('Enter');
  await expect(window.locator('.crumbs button').last()).toHaveText('Mis cosas');
  await expect(window.locator('.empty')).toContainText('vacía');
});

test('a cover is chosen from the images only: the rest stay out of the way until asked', async ({ page }) => {
  await page.goto('/#/new-playlist');
  await page.locator('.url-cover').click();
  const window = explorer(page);
  await expect(window).toBeVisible();
  await expect(window.getByRole('button', { name: 'Usar esta imagen' })).toBeDisabled();

  await window.locator('.xp-place', { hasText: 'Descargas' }).click();
  await expect(window.locator('.xp-row', { hasText: 'portada-verano.png' })).toBeVisible();
  await expect(window.locator('.xp-row', { hasText: 'factura.pdf' })).toHaveCount(0);
  await window.getByRole('button', { name: 'Mostrar todos' }).click();
  await expect(window.locator('.xp-row.dim', { hasText: 'factura.pdf' })).toBeVisible();

  await window.locator('.xp-row', { hasText: 'portada-verano.png' }).click();
  await expect(window.locator('.xp-details')).toContainText('portada-verano.png');
  await window.getByRole('button', { name: 'Usar esta imagen' }).click();
  await expect(window).toBeHidden();
  await expect(page.locator('.cover-x')).toBeVisible();
});

test('closing it without choosing leaves the cover as it was', async ({ page }) => {
  await page.goto('/#/new-playlist');
  await page.locator('.url-cover').click();
  await explorer(page).getByRole('button', { name: 'Cancelar' }).click();
  await expect(explorer(page)).toBeHidden();
  await expect(page.locator('.cover-x')).toBeHidden();
});

test('the phone shows up in the same window, with its folders and a copy target', async ({ page }) => {
  await page.goto('/#/download');
  await page.locator('.dest', { hasText: 'Dispositivo MTP' }).click();
  const window = explorer(page);
  await expect(window.locator('.crumbs')).toContainText('Pixel 8');
  await window.locator('.xp-row', { hasText: 'Music' }).click();
  await expect(window.locator('.mtp-foot')).toContainText('Pixel 8 › Music');
  await window.getByRole('button', { name: 'Guardar aquí' }).click();
  await expect(page.locator('.dest.on small')).toHaveText('Pixel 8 / Music');
});

test('the keyboard walks the list: arrows move, Intro opens, Retroceso goes up', async ({ page }) => {
  await openDestination(page);
  const window = explorer(page);
  await window.locator('.xp-place', { hasText: 'Disco (D:)' }).click();
  await window.locator('.browser-content').focus();
  await page.keyboard.press('ArrowDown');
  await expect(window.locator('.xp-row.cur')).toContainText('Copias');
  await page.keyboard.press('ArrowDown');
  await expect(window.locator('.xp-row.cur')).toContainText('Música');
  await page.keyboard.press('Enter');
  await expect(window.locator('.crumbs button').last()).toHaveText('Música');
  await page.keyboard.press('Backspace');
  await expect(window.locator('.crumbs button').last()).toHaveText('D:');
});

// === FASE 3: FAVORITAS, RECIENTES Y BUSCAR EN SUBCARPETAS ===

test('a starred folder is in the rail the next time, and the star takes it out again', async ({ page }) => {
  await openDestination(page);
  let window = explorer(page);
  await window.locator('.xp-place', { hasText: 'Disco (D:)' }).click();
  await window.locator('.xp-row', { hasText: 'Música' }).click();
  await window.getByRole('button', { name: 'Añadir a favoritas' }).click();
  await expect(window.locator('.xp-place', { hasText: 'Favoritas' })).toHaveCount(0);
  await expect(window.locator('.rail-label', { hasText: 'Favoritas' })).toBeVisible();
  await expect(window.locator('.xp-place', { hasText: 'D:\\Música' })).toBeVisible();

  await window.getByRole('button', { name: 'Cancelar' }).click();
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  window = explorer(page);
  await expect(window.locator('.xp-place', { hasText: 'D:\\Música' })).toBeVisible();
  await window.locator('.xp-place', { hasText: 'D:\\Música' }).click();
  await window.getByRole('button', { name: 'Quitar de favoritas' }).click();
  await expect(window.locator('.rail-label', { hasText: 'Favoritas' })).toHaveCount(0);
});

test('the next window starts in the folder chosen last', async ({ page }) => {
  await openDestination(page);
  let window = explorer(page);
  await window.locator('.xp-place', { hasText: 'Disco (D:)' }).click();
  await window.locator('.xp-row', { hasText: 'Música' }).click();
  await window.getByRole('button', { name: 'Guardar aquí' }).click();
  await expect(explorer(page)).toBeHidden();

  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  window = explorer(page);
  await expect(window.locator('.mtp-foot')).toContainText('D:\\Música');
  await expect(window.locator('.rail-label', { hasText: 'Recientes' })).toBeVisible();
});

test('importing: when a folder shows no playlist the subfolders can be searched and several imported at once', async ({ page }) => {
  await page.goto('/#/manager');
  await page.getByRole('button', { name: 'Importar' }).click();
  await page.getByRole('button', { name: 'Explorar archivos' }).click();
  const window = explorer(page);
  await window.locator('.xp-place', { hasText: 'Disco (D:)' }).click();
  await expect(window.locator('.banner')).toContainText('ninguna playlist');
  await window.getByRole('button', { name: 'Buscar en subcarpetas' }).click();

  await expect(window.locator('.banner')).toContainText('2 playlists');
  await expect(window.locator('.xp-row', { hasText: 'Lo-fi para estudiar' })).toContainText('Música');
  await window.locator('.xp-row', { hasText: 'Bandas sonoras' }).click();
  await expect(window.getByRole('button', { name: 'Importar (1)' })).toBeEnabled();
  await window.locator('.xp-row', { hasText: 'Bandas sonoras' }).click();
  await window.getByRole('button', { name: 'Importar (2)' }).click();

  await expect(explorer(page)).toBeHidden();
  await expect(page.getByRole('dialog')).toContainText('Lo-fi para estudiar');
});

test('searching the subfolders with nothing found says so and goes back', async ({ page }) => {
  await page.goto('/#/manager');
  await page.getByRole('button', { name: 'Importar' }).click();
  await page.getByRole('button', { name: 'Explorar archivos' }).click();
  const window = explorer(page);
  await window.locator('.xp-place', { hasText: 'Descargas' }).click();
  await window.getByRole('button', { name: 'Buscar en subcarpetas' }).click();
  await expect(window.locator('.empty')).toContainText('No hay playlists');
  await window.getByRole('button', { name: 'Volver a la carpeta' }).click();
  await expect(window.locator('.banner')).toContainText('ninguna playlist');
  await expect(window.locator('.crumbs button').last()).toHaveText('Downloads');
});

// === FASE 2: ANDROID ===

test('on a phone the same explorer shows the storages, and the SD card is only read', async ({ page }) => {
  await page.goto('/?mockAndroid#/download');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  const window = explorer(page);
  await expect(window.locator('.xp-place', { hasText: 'Almacenamiento interno' })).toContainText('libres');
  // No USB phones to look for from a phone.
  await expect(window.locator('.rail-label', { hasText: 'Teléfonos' })).toHaveCount(0);
  await expect(window.locator('.mtp-foot')).toContainText('/storage/emulated/0/Music');
  await expect(window.getByRole('button', { name: 'Guardar aquí' })).toBeEnabled();

  await window.locator('.xp-place', { hasText: 'Tarjeta SD' }).click();
  await expect(window.locator('.xp-place', { hasText: 'Tarjeta SD' })).toContainText('solo lectura');
  await expect(window.locator('.banner.warn-b')).toContainText('solo deja leer');
  await expect(window.getByRole('button', { name: 'Guardar aquí' })).toBeDisabled();
  await expect(window.getByRole('button', { name: 'Nueva carpeta' })).toBeDisabled();
});

test('on a phone, Android/data is blocked and the SD card can still be imported from', async ({ page }) => {
  await page.goto('/?mockAndroid#/manager');
  await page.getByRole('button', { name: 'Importar' }).click();
  await page.getByRole('button', { name: 'Explorar archivos' }).click();
  const window = explorer(page);
  await window.locator('.xp-place', { hasText: 'Tarjeta SD' }).click();
  await window.locator('.xp-row', { hasText: 'Music' }).click();
  await expect(window.getByRole('button', { name: 'Buscar aquí' })).toBeEnabled();
});

test('on a phone without the access to files the explorer asks for it, and lists once it is given', async ({ page }) => {
  await page.goto('/?mockAndroid&mockNoStorage#/download');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  const window = explorer(page);
  await expect(window.locator('.empty')).toContainText('necesita acceso');
  await expect(window.getByRole('button', { name: 'Guardar aquí' })).toBeDisabled();
  await window.getByRole('button', { name: 'Dar acceso' }).click();
  await expect(window.locator('.xp-row', { hasText: 'Soundify' })).toBeVisible();
  await expect(window.getByRole('button', { name: 'Guardar aquí' })).toBeEnabled();
});
