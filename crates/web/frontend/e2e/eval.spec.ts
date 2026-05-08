import { expect, test } from '@playwright/test';

test('terminal eval round-trip', async ({ page }) => {
  await page.goto('/');
  // Terminal lives in a dockview tab; click the tab header first so the panel
  // body (and its input) actually mounts in the DOM.
  await page
    .locator('.dv-tab')
    .filter({ hasText: /^Terminal$/ })
    .first()
    .click();
  const input = page.getByTestId('terminal-input');
  await expect(input).toBeVisible();
  await input.fill('block.number');
  await input.press('Enter');
  await expect(page.getByTestId('term-result')).toBeVisible();
});
