import { expect, test } from '@playwright/test';

// Picks an arbitrary file from the explorer (the mock seeds at least one
// address) and confirms the editor opens a tab for it.
test('clicking a file in the explorer opens it in a tab', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('file-explorer')).toBeVisible();

  // The mock fixture's CALL trace expands the root address by default, so
  // the first `explorer-file-*` button is a real file row, not a collapsed
  // address node we'd have to expand first.
  const fileBtn = page.locator('[data-testid^="explorer-file-"]').first();
  await expect(fileBtn).toBeVisible();
  await fileBtn.click();

  // After clicking, dockview mounts a `file` panel inside the editor area.
  // The dockview tab header should carry the file's basename.
  await expect(page.getByTestId('editor-area')).toBeVisible();
  await expect(
    page.locator('.dv-tab').filter({ hasText: /Vault\.sol/ }).first(),
  ).toBeVisible({ timeout: 10_000 });
});
