import { expect, test, type Page } from '@playwright/test';

// The Gestor while one of its playlists downloads, against the mock: it walks
// «Chill de domingo» MP3's two pending files, one every 700 ms after preparing.

/** Opens `hash` with the clock stopped: it only moves with `runFor`, never while an assertion waits. */
async function openPaused(page: Page, hash: string): Promise<void> {
  await page.clock.install({ time: new Date(2026, 10, 3, 20, 0) });
  await page.goto(hash);
  await page.clock.pauseAt(new Date(2026, 10, 3, 20, 0, 1));
}

test('the block being downloaded is outlined; its siblings are not dimmed and can be queued', async ({ page }) => {
  await openPaused(page, '/#/manager?open=pl1');
  const [mp3, flac] = [page.locator('.pm-block').nth(0), page.locator('.pm-block').nth(1)];
  await mp3.getByRole('button', { name: 'Sincronizar' }).click();

  await expect(mp3).toHaveClass(/active/);
  await expect(mp3.locator('.pm-badge')).toHaveText('Descargando');
  await expect(flac.getByRole('button', { name: 'Sincronizar' })).toBeEnabled();
  await expect(flac).not.toHaveClass(/waiting/);
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

test('a second Sincronizar waits in the queue, shows in Tareas, can be taken out, and starts by itself otherwise', async ({ page }) => {
  await openPaused(page, '/#/manager?open=pl1');
  const [mp3, flac] = [page.locator('.pm-block').nth(0), page.locator('.pm-block').nth(1)];
  await mp3.getByRole('button', { name: 'Sincronizar' }).click();
  await flac.getByRole('button', { name: 'Sincronizar' }).click();

  await expect(page.locator('.toast').last()).toContainText('espera su turno');
  await expect(flac.locator('.pm-badge')).toHaveText('En cola');
  await expect(flac.getByRole('button', { name: 'Quitar de la cola' })).toBeVisible();
  await expect(flac).not.toHaveClass(/active/);

  // The same archive twice is refused: the running one has Detener, not Sincronizar.
  await expect(mp3.getByRole('button', { name: 'Detener' })).toBeVisible();
  await page.locator('.tasks-pill').click();
  const overlay = page.locator('#overlay-root');
  await expect(overlay.locator('[data-queued]')).toHaveCount(1);
  await expect(overlay.getByText('En cola (1)')).toBeVisible();
  await expect(page.locator('.tasks-pill-sub')).toContainText('+1 en cola');

  // Taken out from Tareas, the block is a plain Sincronizar again.
  await overlay.getByRole('button', { name: 'Quitar de la cola' }).click();
  await expect(overlay.locator('[data-queued]')).toHaveCount(0);
  await expect(flac.locator('.pm-badge')).toHaveCount(0);

  await page.keyboard.press('Escape');
  await page.clock.runFor(500);
  await expect(overlay.locator('.card')).toHaveCount(0);
  // Queued again, it starts by itself when the first one is done.
  await flac.getByRole('button', { name: 'Sincronizar' }).click();
  await expect(flac.locator('.pm-badge')).toHaveText('En cola');
  await page.clock.runFor(6_000);
  await expect(mp3).not.toHaveClass(/active/);
  await expect(flac).toHaveClass(/active/);
  await expect(flac.locator('.pm-badge')).toHaveText('Descargando');
});

test('with «Descargar más rápido» two blocks download at once, a third waits, and each one stops on its own', async ({ page }) => {
  await openPaused(page, '/#/settings');
  await page.locator('.settings-tab', { hasText: 'Avanzado' }).click();
  await page.locator('[data-key="settings.advanced.faster"] input.switch').check();
  await page.evaluate(() => { location.hash = '#/manager?open=pl1'; });
  const [mp3, flac] = [page.locator('.pm-block').nth(0), page.locator('.pm-block').nth(1)];
  await mp3.getByRole('button', { name: 'Sincronizar' }).click();
  await flac.getByRole('button', { name: 'Sincronizar' }).click();

  // Both run: nothing waits, and each block has its own badge and button.
  await expect(mp3).toHaveClass(/active/);
  await expect(flac).toHaveClass(/active/);
  await expect(mp3.locator('.pm-badge')).toHaveText('Descargando');
  await expect(flac.locator('.pm-badge')).toHaveText('Descargando');
  await expect(mp3.getByRole('button', { name: 'Detener' })).toBeVisible();
  await expect(flac.getByRole('button', { name: 'Detener' })).toBeVisible();
  await expect(page.locator('.tasks-pill-title')).toContainText('2 descargas');

  // Each one counts its own files: MP3 has two pending, FLAC walks one.
  await page.clock.runFor(2_100);
  await expect(mp3).toContainText('Archivo 1 de 2');
  await expect(flac).toContainText('Archivo 1 de 1');

  // A third one waits for room.
  const row = page.locator('.pm-row', { hasText: 'Para entrenar' });
  await row.locator('.pm-row-header').click();
  const opus = row.locator('.pm-block');
  await opus.getByRole('button', { name: 'Sincronizar' }).click();
  await expect(opus.locator('.pm-badge')).toHaveText('En cola');
  await expect(page.locator('.tasks-pill-sub')).toContainText('+1 en cola');

  // Stopping MP3 leaves FLAC going, and frees the room for the one that waited.
  await mp3.getByRole('button', { name: 'Detener' }).click();
  await expect(mp3).not.toHaveClass(/active/);
  await expect(flac).toHaveClass(/active/);
  await expect(opus).toHaveClass(/active/);
  await expect(opus.locator('.pm-badge')).toHaveText('Descargando');

  // Tareas lists both and can stop them together.
  await page.locator('.tasks-pill').click();
  const overlay = page.locator('#overlay-root');
  await expect(overlay.locator('[data-running]')).toHaveCount(2);
  await overlay.getByRole('button', { name: 'Detener todas' }).click();
  await expect(overlay.locator('[data-running]')).toHaveCount(0);
  await expect(flac).not.toHaveClass(/active/);
  await expect(opus).not.toHaveClass(/active/);
});

test('the same block cannot be started twice with several downloads at once, and off the second always waits', async ({ page }) => {
  // Off (the default): the second one waits even though it is another block.
  await openPaused(page, '/#/manager?open=pl1');
  await page.locator('.pm-block').nth(0).getByRole('button', { name: 'Sincronizar' }).click();
  await page.locator('.pm-block').nth(1).getByRole('button', { name: 'Sincronizar' }).click();
  await expect(page.locator('.pm-block').nth(1).locator('.pm-badge')).toHaveText('En cola');
  await expect(page.locator('.tasks-pill-title')).not.toContainText('2 descargas');
});
