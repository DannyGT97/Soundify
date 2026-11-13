import { expect, test, type Page } from '@playwright/test';

// Every screen and how the menu moves between them (the Java app's
// FxmlLoadUiTest and NavigationUiTest).

const SCREENS = [
  { route: 'home', title: 'Inicio' },
  { route: 'download', title: 'Descarga archivos por URL' },
  { route: 'new-playlist', title: 'Nueva Playlist' },
  { route: 'manager', title: 'Gestor de Playlists' },
  { route: 'player', title: 'Reproductor' },
  { route: 'now-playing', title: 'Reproduciendo' },
  { route: 'settings', title: 'Ajustes' },
];

/** Fails the test on any uncaught error in the page: a screen that throws while building. */
function failOnPageErrors(page: Page): void {
  page.on('pageerror', error => { throw error; });
}

for (const { route, title } of SCREENS) {
  test(`the ${route} screen loads with its title`, async ({ page }) => {
    failOnPageErrors(page);
    await page.goto(`/#/${route}`);
    // Reproduciendo has no title: it has the way back instead.
    if (route === 'now-playing') await expect(page.locator('#page .np-back')).toBeVisible();
    else await expect(page.locator('#page .page-title').first()).toHaveText(title);
  });
}

test('the menu leads to every screen and marks the one shown', async ({ page }) => {
  failOnPageErrors(page);
  await page.goto('/#/home');
  for (const label of ['Descarga por URL', 'Crear Playlist', 'Gestor de Playlists', 'Reproductor', 'Ajustes', 'Inicio']) {
    const button = page.locator('.sidebar .nav-button', { hasText: label });
    await button.click();
    await expect(button).toHaveClass(/selected/);
    await expect(page.locator('.sidebar .nav-button.selected')).toHaveCount(1);
  }
});

test('the folded menu stays folded when changing screens', async ({ page }) => {
  await page.goto('/#/home');
  const sidebar = page.locator('.sidebar');
  await page.locator('.sidebar-toggle').click();
  await expect(sidebar).toHaveClass(/collapsed/);
  await page.locator('.sidebar .nav-button[title="Gestor de Playlists"]').click();
  await expect(page).toHaveURL(/#\/manager/);
  await expect(sidebar).toHaveClass(/collapsed/);
});

test('an unknown address falls back to Inicio', async ({ page }) => {
  await page.goto('/#/nowhere');
  await expect(page.locator('#page .page-title').first()).toHaveText('Inicio');
});

test('the title bar: only where there is a window, with labelled buttons reachable by keyboard', async ({ page }) => {
  await page.goto('/#/home');
  await expect(page.locator('.titlebar')).toHaveCount(0);

  await page.goto('/?mockWindow#/home');
  const bar = page.locator('.titlebar');
  await expect(bar.getByRole('button')).toHaveCount(3);
  await bar.getByRole('button', { name: 'Minimizar' }).focus();
  await page.keyboard.press('Tab');
  await expect(bar.getByRole('button', { name: 'Maximizar' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(bar.getByRole('button', { name: 'Restaurar' })).toBeVisible();
  await bar.getByRole('button', { name: 'Restaurar' }).click();
  await expect(bar.getByRole('button', { name: 'Maximizar' })).toBeVisible();
  await expect(bar.getByRole('button', { name: 'Cerrar' })).toBeVisible();
});
