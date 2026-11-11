import { expect, test, type Page } from '@playwright/test';

// Playback through the PlayerBackend (the simulated one in the mock): the bar
// appears, the clock moves and ✕ unloads it (the Java app's PlayerUiTest).

test('playing a file shows the bar and moves the clock; closing hides it', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  const bar = page.locator('.player-bar');
  await expect(bar).toBeHidden();

  // Pressing a file plays it and stays on the list; Reproduciendo opens from the bar.
  await page.locator('.track-row').first().click();
  await expect(page).toHaveURL(/#\/player/);
  await expect(bar).toBeVisible();
  await expect(bar.locator('.player-time').first()).toHaveText('0:00');

  await page.clock.runFor(3000);
  await expect(bar.locator('.player-time').first()).toHaveText('0:03');

  await bar.locator('.player-play').click();
  await page.clock.runFor(3000);
  await expect(bar.locator('.player-time').first()).toHaveText('0:03');

  await bar.locator('.player-control[title="Cerrar el reproductor"]').click();
  await expect(bar).toBeHidden();
});

test('the bar: duration once opened, next moves on, repeat cycles and previous restarts', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  const titles = await page.locator('.track-row .row-title').allTextContents();
  await page.locator('.track-row').first().click();
  const bar = page.locator('.player-bar');
  const [position, duration] = [bar.locator('.player-time').first(), bar.locator('.player-time').last()];
  // The length comes from the engine once the file is open, not from the archive.
  await page.clock.runFor(100);
  await expect(duration).not.toHaveText('0:00');
  await expect(bar.locator('.player-now .row-title')).toHaveText(titles[0] ?? '');

  await bar.locator('.player-control[title="Siguiente"]').click();
  await expect(bar.locator('.player-now .row-title')).toHaveText(titles[1] ?? '');

  const repeat = bar.locator('.player-control[title^="Repetir"]');
  await expect(repeat).toHaveAttribute('title', 'Repetir: no');
  await repeat.click();
  await expect(repeat).toHaveAttribute('title', 'Repetir: toda la playlist');
  await repeat.click();
  await expect(repeat).toHaveAttribute('title', 'Repetir: este archivo');
  await repeat.click();
  await expect(repeat).toHaveAttribute('title', 'Repetir: no');

  // Past the first seconds "previous" restarts the file instead of going back.
  await page.clock.runFor(5000);
  await expect(position).toHaveText('0:05');
  await bar.locator('.player-control[title="Anterior"]').click();
  await expect(position).toHaveText('0:00');
  await expect(bar.locator('.player-now .row-title')).toHaveText(titles[1] ?? '');
});

test('a download from the URL screen shows in the menu indicator', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/download');
  await page.locator('input.input').first().fill('https://www.youtube.com/watch?v=abc');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  await page.getByRole('button', { name: 'Descargar', exact: true }).click();
  await expect(page.locator('.tasks-pill')).toBeVisible();
  await page.clock.runFor(10_000);
  await expect(page.locator('.tasks-pill-title')).toHaveText('Terminada');
});

test('Descarga por URL refuses a list, even a video opened from inside one, and points to Crear Playlist', async ({ page }) => {
  await page.goto('/#/download');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  const url = page.locator('input.input').first();
  const start = page.getByRole('button', { name: 'Descargar', exact: true });

  await url.fill('https://www.youtube.com/watch?v=abc&list=PL123');
  await expect(start).toBeDisabled();
  await page.getByRole('button', { name: 'Llevarla a Crear Playlist' }).click();
  await expect(page).toHaveURL(/#\/new-playlist/);
  await expect(page.locator('input.input').first()).toHaveValue('https://www.youtube.com/watch?v=abc&list=PL123');

  await page.goto('/#/download');
  await page.locator('.dest', { hasText: 'Carpeta local' }).click();
  await page.locator('input.input').first().fill('https://www.youtube.com/watch?v=abc');
  await expect(page.getByRole('button', { name: 'Descargar', exact: true })).toBeEnabled();
});

test('regression: clicking the file already playing pauses it, it does not start it again', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const position = page.locator('.player-bar .player-time').first();
  await page.clock.runFor(4000);
  await expect(position).toHaveText('0:04');

  await page.locator('.track-row').first().click();
  await page.clock.runFor(2000);
  await expect(position).toHaveText('0:04');

  await page.locator('.player-bar .player-now').click();
  await expect(page).toHaveURL(/#\/now-playing/);
  await expect(position).toHaveText('0:04');
});

test('regression: pressing play does not rebuild the rows (their thumbnails used to blink)', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  const rows = page.locator('.track-row');
  await rows.first().waitFor();
  await rows.evaluateAll(all => all.forEach((row, i) => { (row as HTMLElement & { mark?: number }).mark = i + 1; }));

  await rows.nth(1).click();
  await page.clock.runFor(2000);
  await expect(rows.nth(1)).toHaveClass(/playing/);
  await page.locator('.player-bar .player-control[title="Siguiente"]').click();
  await page.clock.runFor(1000);
  await expect(rows.nth(2)).toHaveClass(/playing/);
  await expect(rows.nth(1)).not.toHaveClass(/playing/);

  // The same nodes as before: nothing was replaced, so no picture was dropped and loaded again.
  const marks = await rows.evaluateAll(all => all.map(row => (row as HTMLElement & { mark?: number }).mark));
  expect(marks).toEqual(marks.map((_, i) => i + 1));
});

test('the headers sort the files and the queue follows the order shown', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  const rows = page.locator('.track-row');
  await rows.first().waitFor();
  const numbers = () => rows.locator('.num, .eq').evaluateAll(all => all.map(el => el.textContent));
  const ascending = await numbers();
  const titleHeader = page.locator('.cols .col-sort', { hasText: 'Título' });
  const numberHeader = page.locator('.cols .col-sort.n');
  await expect(numberHeader).toHaveAttribute('aria-sort', 'ascending');

  // The active column flips when pressed again; the first row is the last number.
  await numberHeader.click();
  await expect(numberHeader).toHaveAttribute('aria-sort', 'descending');
  const descending = await numbers();
  expect(descending).toEqual([...ascending].reverse());

  // Another column starts ascending and the previous one stops being marked.
  await titleHeader.click();
  await expect(titleHeader).toHaveAttribute('aria-sort', 'ascending');
  await expect(numberHeader).toHaveAttribute('aria-sort', 'none');
  const titles = await rows.locator('.row-title').allTextContents();
  expect(titles).toEqual([...titles].sort((a, b) => a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' })));

  // «Descargado» too, and pressing the first file plays the one shown first.
  await page.locator('.cols .col-sort.added').click();
  await expect(page.locator('.cols .col-sort.added')).toHaveAttribute('aria-sort', 'ascending');
  const first = (await rows.locator('.row-title').first().textContent()) ?? '';
  await rows.first().click();
  await page.clock.runFor(1000);
  await expect(page.locator('.player-bar')).toContainText(first);
});

test('regression: space plays and pauses instead of pressing the last button again', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const bar = page.locator('.player-bar');
  const position = bar.locator('.player-time').first();

  // The focus stays on «Siguiente»: space must pause, not skip to another file.
  await bar.locator('.player-control[title="Siguiente"]').click();
  const title = await bar.locator('.player-now .row-title').textContent();
  await page.clock.runFor(2000);
  await expect(position).toHaveText('0:02');
  await page.keyboard.press('Space');
  await page.clock.runFor(3000);
  await expect(position).toHaveText('0:02');
  await expect(bar.locator('.player-now .row-title')).toHaveText(title ?? '');

  await page.keyboard.press('Space');
  await page.clock.runFor(2000);
  await expect(position).toHaveText('0:04');
});

test('with the player only in the side menu, Reproduciendo has the volume', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Interfaz' }).click();
  await page.locator('select').first().selectOption('side');
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  await expect(page.locator('.player-bar')).toBeHidden();
  await page.locator('.mini-player b.player-link').click();
  await expect(page.locator('.np-controls .player-volume .player-slider')).toBeVisible();
});

test('the mini player seeks by click, drag and arrows, without opening anything or toggling play', async ({ page }) => {
  await page.goto('/#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  await page.goto('/#/home');
  const mini = page.locator('.mini-player');
  const seek = mini.getByRole('slider', { name: 'Posición' });
  await expect(seek).not.toHaveAttribute('max', '0');
  const box = await seek.boundingBox();
  if (!box) throw new Error('no slider');
  const at = (fraction: number) => ({ x: box.x + box.width * fraction, y: box.y + box.height / 2 });
  const max = Number(await seek.getAttribute('max'));

  await page.mouse.click(at(0.5).x, at(0.5).y);
  await expect(page).toHaveURL(/#\/home/);
  const clicked = Number(await seek.inputValue()) / max;
  expect(clicked).toBeGreaterThan(0.4);
  expect(clicked).toBeLessThan(0.6);
  await expect(mini.locator('.mini-play')).toHaveAttribute('title', 'Pausa');

  await page.mouse.move(at(0.5).x, at(0.5).y);
  await page.mouse.down();
  await page.mouse.move(at(0.8).x, at(0.8).y, { steps: 4 });
  await page.mouse.up();
  expect(Number(await seek.inputValue()) / max).toBeGreaterThan(0.7);

  await mini.locator('.mini-play').click();
  await seek.focus();
  const before = Number(await seek.inputValue());
  await page.keyboard.press('ArrowLeft');
  await expect(seek).toHaveValue(String(before - 1));
});

test.describe('picture in picture', () => {
  const playVideo = async (page: Page) => {
    await page.goto('/#/player?b=pl3:MP4');
    await page.locator('.track-row').first().click();
    await page.locator('.player-bar .player-now').click();
    await expect(page).toHaveURL(/#\/now-playing/);
    // The screen is drawn on the hashchange event, a moment after the address changes.
    await expect(page.locator('.np-art')).toBeVisible();
  };
  const panel = (page: Page) => page.locator('.pip-panel');

  test('only a video offers it', async ({ page }) => {
    await page.goto('/#/player?b=pl1:MP3');
    await page.locator('.track-row').first().click();
    await expect(page.locator('.player-bar')).toBeVisible();
    await expect(page.locator('.pip-button:visible')).toHaveCount(0);

    await playVideo(page);
    await expect(page.locator('.np-art .pip-button')).toBeVisible();
    await expect(page.locator('.mini-player .pip-button')).toBeVisible();
    await expect(page.locator('.player-bar .pip-button')).toBeVisible();
  });

  test('it floats over every screen, playing; ✕ pauses at the same second and keeps the file', async ({ page }) => {
    await playVideo(page);
    await page.locator('.np-art .pip-button').click();
    await expect(panel(page)).toBeVisible();
    await expect(page.locator('.np-pip-here')).toBeVisible();

    await page.goto('/#/manager');
    await expect(panel(page)).toBeVisible();
    await expect(page.locator('.mini-player .mini-play')).toHaveAttribute('title', 'Pausa');

    await panel(page).hover();
    await panel(page).getByRole('button', { name: 'Cerrar y pausar' }).click();
    await expect(panel(page)).toBeHidden();
    await expect(page.locator('.player-bar')).toBeVisible();
    const play = page.locator('.mini-player .mini-play');
    await expect(play).toHaveAttribute('title', 'Reproducir');
    const stopped = await page.locator('.player-bar .player-time').first().textContent();
    await page.waitForTimeout(1200);
    await expect(page.locator('.player-bar .player-time').first()).toHaveText(stopped ?? '');
  });

  test('it can be dragged, inside the window', async ({ page }) => {
    await playVideo(page);
    await page.locator('.np-art .pip-button').click();
    // Over Reproduciendo the placeholder would take it back in (see the docking test): drag elsewhere.
    await page.goto('/#/home');
    const box = await panel(page).boundingBox();
    if (!box) throw new Error('no panel');
    await page.mouse.move(box.x + 100, box.y + 100);
    await page.mouse.down();
    await page.mouse.move(box.x - 200, box.y - 150, { steps: 4 });
    await page.mouse.up();
    const moved = await panel(page).boundingBox();
    expect(moved?.x).toBeCloseTo(box.x - 300, 0);
    await page.mouse.move(box.x - 200, box.y - 150);
    await page.mouse.down();
    await page.mouse.move(-500, -500, { steps: 4 });
    await page.mouse.up();
    const corner = await panel(page).boundingBox();
    expect(corner?.x).toBeGreaterThanOrEqual(0);
    expect(corner?.y).toBeGreaterThanOrEqual(0);
  });

  test('expanding goes back to Reproduciendo, still playing', async ({ page }) => {
    await playVideo(page);
    await page.locator('.np-art .pip-button').click();
    await page.goto('/#/home');
    await panel(page).hover();
    await panel(page).getByRole('button', { name: 'Volver a Reproduciendo' }).click();
    await expect(page).toHaveURL(/#\/now-playing/);
    await expect(panel(page)).toBeHidden();
    await expect(page.locator('.np-pip-here')).toHaveCount(0);
    await expect(page.locator('.mini-player .mini-play')).toHaveAttribute('title', 'Pausa');
  });

  test('another playlist, of audio, takes the panel away and keeps sounding', async ({ page }) => {
    await playVideo(page);
    await page.locator('.np-art .pip-button').click();
    await page.goto('/#/player?b=pl1:MP3');
    await page.locator('.track-row').first().click();
    await expect(panel(page)).toBeHidden();
    await expect(page.locator('.mini-player .mini-play')).toHaveAttribute('title', 'Pausa');
  });

  test('leaving Reproduciendo floats a video by itself, and coming back puts it in place', async ({ page }) => {
    await playVideo(page);
    await expect(panel(page)).toBeHidden();
    await page.goto('/#/home');
    await expect(panel(page)).toBeVisible();
    await page.goto('/#/now-playing');
    await expect(panel(page)).toBeHidden();
    await expect(page.locator('.np-pip-here')).toHaveCount(0);
  });

  test('the switch in Reproduciendo turns the automatic panel off; a manual one stays', async ({ page }) => {
    await playVideo(page);
    const chip = page.locator('.pip-auto');
    await expect(chip).toHaveAttribute('aria-pressed', 'true');
    await chip.click();
    await expect(chip).toHaveAttribute('aria-pressed', 'false');
    await page.goto('/#/home');
    await expect(panel(page)).toBeHidden();

    await page.goto('/#/now-playing');
    await page.locator('.np-art .pip-button').click();
    await page.goto('/#/home');
    await expect(panel(page)).toBeVisible();
  });

  test('audio never floats', async ({ page }) => {
    await page.goto('/#/player?b=pl1:MP3');
    await page.locator('.track-row').first().click();
    await page.locator('.player-bar .player-now').click();
    await expect(page.locator('.pip-auto')).toBeHidden();
    await page.goto('/#/home');
    await expect(panel(page)).toBeHidden();
  });

  test('the placeholder is small text, and dragging the panel onto it docks it back', async ({ page }) => {
    await playVideo(page);
    await page.locator('.np-art .pip-button').click();
    const here = page.locator('.np-pip-here');
    await expect(here).toBeVisible();
    const size = await here.locator('.pip-here-text').evaluate(el => parseFloat(getComputedStyle(el).fontSize));
    expect(size).toBeLessThanOrEqual(16);

    const spot = await here.boundingBox();
    const box = await panel(page).boundingBox();
    if (!spot || !box) throw new Error('missing boxes');
    await page.mouse.move(box.x + 60, box.y + 60);
    await page.mouse.down();
    await page.mouse.move(spot.x + spot.width / 2, spot.y + spot.height / 2, { steps: 6 });
    await expect(here).toHaveClass(/dock-over/);
    await page.mouse.up();
    await expect(panel(page)).toBeHidden();
    await expect(page.locator('.np-pip-here')).toHaveCount(0);
  });
});

test('the big cover of a playlist shows a pencil on hover and is editable', async ({ page }) => {
  await page.goto('/#/player?b=pl1:MP3');
  const cover = page.locator('.hero-cover');
  const shade = cover.locator('.shade');
  await expect(cover).toHaveAttribute('role', 'button');
  await expect(shade).toHaveCSS('opacity', '0');
  await cover.hover();
  await expect(shade).toHaveCSS('opacity', '1');
  await expect(shade.locator('svg')).toBeVisible();
  // The mock's picker returns no image: the click does nothing but must not break the view.
  await cover.click();
  await expect(page.locator('.hero h1')).toBeVisible();
});
