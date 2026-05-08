import { expect, test } from '@playwright/test';

test('terminal eval round-trip', async ({ page }) => {
  await page.goto('/');
  const input = page.getByTestId('terminal-input');
  await input.fill('block.number');
  await input.press('Enter');
  await expect(page.getByTestId('term-result')).toBeVisible();
});
