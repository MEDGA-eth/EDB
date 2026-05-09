import { expect, test } from '@playwright/test';

// The Display panel lives as a tab inside the bottom dockview group. We
// switch to its tab, then to its inner "Storage" sub-tab, and assert that
// the mock fixture (storageDiff-0.json — 2 rows) renders.
test('storage diff sub-tab renders rows from the engine response', async ({ page }) => {
  await page.goto('/');

  // Click the "Display" dockview tab header at the top of the bottom group.
  await page
    .locator('.dv-tab')
    .filter({ hasText: /^Display$/ })
    .first()
    .click();

  // Switch to the Storage sub-tab inside DisplayPanel.
  await page.getByTestId('display-tab-storage').click();

  // The mock seeds 2 rows with both before/after fields; assert the first.
  const firstRow = page.getByTestId('storage-row-0');
  await expect(firstRow).toBeVisible();
});
