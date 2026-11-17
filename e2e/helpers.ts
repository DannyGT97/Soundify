import type { Page } from '@playwright/test';

/** The destination card of a download form: the explorer opens on Música and its button takes that folder. */
export async function pickLocalDestination(page: Page): Promise<void> {
  await page.locator('.dest').click();
  await page.getByRole('dialog').getByRole('button', { name: 'Guardar aquí' }).click();
}

/** The explorer window, whatever it is open for. */
export const explorer = (page: Page) => page.locator('.mtp.xp');
