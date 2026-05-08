import { expect, test } from '@playwright/test';

test('help overlay opens and closes', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('help-open').click();
  await expect(page.getByTestId('help-overlay')).toBeVisible();
  await page.getByText('Close').click();
  await expect(page.getByTestId('help-overlay')).toBeHidden();
});
