import { expect, test } from '@playwright/test';

// With the real-engine captures, the trace contains 4 distinct contract
// addresses (router, pair, WETH, intermediate). Each address contributes at
// least one file row (Solidity sources for the captured ones, a `(disassembly)`
// row for the Opcodes-stub fallbacks). We assert both: at least 4 address
// rows are visible, and clicking the first file row opens it as a tab.
test('explorer lists every traced address and opens a file tab on click', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('file-explorer')).toBeVisible();

  // ≥ 4 address rows — one per unique `code_address` in the trace fixture.
  const addrRows = page.locator('[data-testid^="explorer-addr-"]');
  await expect.poll(() => addrRows.count(), { timeout: 10_000 }).toBeGreaterThanOrEqual(4);

  // ≥ 1 file row per address. We can't trivially scope to a specific address
  // without knowing the tree shape, so assert that the total file row count is
  // at least the address count — which holds whenever every address has been
  // expanded with at least one file or a disassembly entry.
  const fileRows = page.locator('[data-testid^="explorer-file-"]');
  await expect.poll(() => fileRows.count(), { timeout: 10_000 }).toBeGreaterThanOrEqual(4);

  // Click the first file row — it's a real Solidity file from the router
  // (path = "Contract"), so the tab title should match.
  const fileBtn = fileRows.first();
  await expect(fileBtn).toBeVisible();
  await fileBtn.click();

  await expect(page.getByTestId('editor-area')).toBeVisible();
  await expect(
    page.locator('.dv-tab').filter({ hasText: /Contract|\(disassembly\)/ }).first(),
  ).toBeVisible({ timeout: 10_000 });
});
