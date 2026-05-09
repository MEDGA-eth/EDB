import { expect, test } from '@playwright/test';

test('help overlay opens and closes', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('help-open').click();
  await expect(page.getByTestId('help-overlay')).toBeVisible();
  // The redesigned overlay has both an ⓧ in the header and a "Close"
  // button in the footer. getByText('Close') matches both — pick the
  // button explicitly via role + last() so the click is unambiguous.
  await page.getByRole('button', { name: 'Close' }).last().click();
  await expect(page.getByTestId('help-overlay')).toBeHidden();
});
