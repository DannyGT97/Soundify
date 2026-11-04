import { expect, test, type Page } from '@playwright/test';

// The Gestor while one of its playlists downloads, against the mock: it walks
// «Chill de domingo» MP3's two pending files, one every 700 ms after preparing.

/** Opens `hash` with the clock stopped: it only moves with `runFor`, never while an assertion waits. */
async function openPaused(page: Page, hash: string): Promise<void> {
  await page.clock.install({ time: new Date(2026, 10, 3, 20, 0) });
  await page.goto(hash);
  await page.clock.pauseAt(new Date(2026, 10, 3, 20, 0, 1));
}

test('the block being downloaded is outlined and its siblings wait', async ({ page }) => {
  await openPaused(page, '/#/manager?open=pl1');
  const [mp3, flac] = [page.locator('.pm-block').nth(0), page.locator('.pm-block').nth(1)];
  await mp3.getByRole('button', { name: 'Sincronizar' }).click();

  await expect(mp3).toHaveClass(/active/);
  await expect(mp3.locator('.pm-badge')).toHaveText('Descargando');
  await expect(flac).toHaveClass(/waiting/);
  await expect(flac.getByRole('button', { name: 'Sincronizar' })).toBeDisabled();
  await expect(page.locator('.pm-row.active .pm-live')).toHaveText('Preparando MP3…');

  await page.clock.runFor(2_100);
  await expect(page.locator('.pm-row.active .pm-live')).toHaveText('Descargando MP3 · 1/2');
  await expect(mp3).toContainText('Archivo 1 de 2');

  // Folded, the row still says it, with its own thin bar.
  await page.locator('.pm-row.active .pm-row-header').click();
  await expect(page.locator('.pm-row.active .pm-row-bar')).toBeVisible();
  await page.locator('.pm-row.active .pm-row-header').click();

  await page.clock.runFor(3_000);
  await expect(page.locator('.pm-row.active')).toHaveCount(0);
  await expect(page.locator('.pm-block').nth(0)).toHaveClass(/done/);
  await expect(page.locator('.pm-block').nth(0).getByRole('button', { name: 'Sincronizar' })).toBeEnabled();
});

test('«Ver archivos» shows each file as it lands', async ({ page }) => {
  await openPaused(page, '/#/manager?open=pl1');
  const mp3 = page.locator('.pm-block').nth(0);
  await mp3.getByRole('button', { name: 'Sincronizar' }).click();
  await mp3.getByRole('button', { name: 'Ver archivos' }).click();
  const pending = page.locator('.fa-list .pill.warn');
  await expect(pending).toHaveCount(2);

  await page.clock.runFor(2_800 + 400);
  await expect(pending).toHaveCount(1);
  await expect(page.locator('.fa-list .track.landed')).toHaveCount(1);

  await page.clock.runFor(700);
  await expect(pending).toHaveCount(0);
});

test('«Ir a la descarga» opens the Gestor on the block being downloaded', async ({ page }) => {
  await openPaused(page, '/#/manager?open=pl4');
  await page.locator('.pm-block').filter({ hasText: 'OPUS' }).getByRole('button', { name: 'Sincronizar' }).click();
  await page.goto('/#/home');
  await page.locator('.tasks-pill').click();
  await page.locator('#overlay-root').getByRole('button', { name: 'Ir a la descarga' }).click();
  await expect(page).toHaveURL(/#\/manager\?open=pl4/);
  // The scroll waits for the next frame and glides over several: all of them come from the paused clock.
  await page.clock.runFor(1_000);
  await expect(page.locator('.pm-block.active')).toBeInViewport();
});
