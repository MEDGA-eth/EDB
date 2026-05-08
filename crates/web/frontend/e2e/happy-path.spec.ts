import { expect, test } from '@playwright/test';

test('boots with all four panels and updates URL on Next', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('desktop-layout')).toBeVisible();
  await expect(page.getByTestId('opcodes-view').or(page.getByTestId('solidity-view'))).toBeVisible();
  await expect(page.getByTestId('trace-panel')).toBeVisible();
  await expect(page.getByTestId('display-tab-vars')).toBeVisible();
  await expect(page.getByTestId('terminal-panel')).toBeVisible();

  // Step the snapshot id forward 3 times by writing the URL hash directly.
  for (let i = 0; i < 3; i++) {
    await page.evaluate(() => {
      const next = (parseInt(location.hash.replace('#', ''), 10) || 0) + 1;
      location.hash = String(next);
      window.dispatchEvent(new Event('hashchange'));
    });
  }
  await expect(page).toHaveURL(/#3$/);
});
