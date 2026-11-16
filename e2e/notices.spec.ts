import { expect, test } from '@playwright/test';

// A notice makes room for itself by pushing the screen down (it never covers the title, the buttons or the
// player), goes away by itself, and stays in the list behind the bell.

async function resetSettings(page: import('@playwright/test').Page): Promise<void> {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Avanzado' }).click();
  await page.getByRole('button', { name: 'Reset', exact: true }).click();
  await page.locator('#overlay-root .danger-button').click();
}

test('a notice pushes the screen down instead of covering it, and goes by itself', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Avanzado' }).click();
  const title = page.locator('.screen-head h1');
  const before = (await title.boundingBox())!.y;
  await page.getByRole('button', { name: 'Reset', exact: true }).click();
  await page.locator('#overlay-root .danger-button').click();
  const toast = page.locator('.toast').filter({ hasText: 'Configuración restablecida' });
  await expect(toast).toBeVisible();
  // The title is still there, further down: nothing got over it.
  await page.clock.runFor(300);
  expect((await title.boundingBox())!.y).toBeGreaterThan(before + 20);
  await expect(page.locator('.activity-bell')).toBeVisible();
  await page.clock.runFor(5_500);
  await expect(toast).toHaveCount(0);
  // The screen glides back up (a CSS transition, which the clock does not drive).
  await expect.poll(async () => Math.abs((await title.boundingBox())!.y - before)).toBeLessThan(2);
});

test('the bell lights up for a notice nobody has seen, and its window lists it and can clear it', async ({ page }) => {
  await resetSettings(page);
  const bell = page.locator('.activity-bell');
  await expect(bell.locator('.activity-dot')).toBeVisible();
  await bell.click();
  const overlay = page.locator('#overlay-root');
  await expect(overlay.locator('.notice-row')).toContainText('Configuración restablecida');
  await page.keyboard.press('Escape');
  // Seen: no dot.
  await expect(bell.locator('.activity-dot')).toBeHidden();
  await bell.click();
  await overlay.getByRole('button', { name: 'Vaciar avisos' }).click();
  await expect(overlay.locator('.notice-row')).toHaveCount(0);
  await expect(overlay.getByText('Sin avisos')).toBeVisible();
});

test('at most three notices show at once; the older ones stay in the list', async ({ page }) => {
  await page.goto('/#/settings');
  await page.locator('.settings-tab', { hasText: 'Avanzado' }).click();
  for (let i = 0; i < 4; i++) {
    await page.getByRole('button', { name: 'Reset', exact: true }).click();
    await page.locator('#overlay-root .danger-button').click();
    await expect(page.locator('#overlay-root .danger-button')).toHaveCount(0);
  }
  await expect(page.locator('.toast-slot:not(.toast-leaving)')).toHaveCount(3);
  await page.locator('.activity-bell').click();
  await expect(page.locator('#overlay-root .notice-row')).toHaveCount(4);
});
