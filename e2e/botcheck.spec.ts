import { expect, test } from '@playwright/test';

// When YouTube's bot check cuts a run, the notice stays on screen and takes the
// user to where the cookies are (`?mockBotCheck` fires it against the mock).

test('the bot-check notice stays and its button opens Ajustes on the Avanzado tab', async ({ page }) => {
  await page.goto('/?mockBotCheck#/home');
  const toast = page.locator('.toast-error');
  await expect(toast).toContainText('no hay cookies');
  await expect(toast).toContainText('Ajustes → Avanzado');

  // It is not a passing notice: still there well after the usual 5 seconds.
  await page.waitForTimeout(5500);
  await expect(toast).toBeVisible();

  await toast.getByRole('button', { name: 'Ir a Ajustes' }).click();
  await expect(page).toHaveURL(/#\/settings/);
  await expect(page.locator('.settings-tab.active')).toContainText('Avanzado');
  await expect(toast).toBeHidden();
});
