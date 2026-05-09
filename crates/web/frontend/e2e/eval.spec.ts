import { expect, test } from '@playwright/test';

// The mock fixture for `edb_evalOnSnapshot` is the real engine response for
// the expression `1 + 1` on snapshot 0: `{ Ok: { type: 'Uint', value: { bits: 256, value: '0x2' } } }`.
// The terminal renders this through `formatEvalResult` → `formatSolValue`,
// which maps Uint to its hex string ("0x2"). We assert both the input echo
// and the rendered result.
test('terminal eval round-trip renders engine result', async ({ page }) => {
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
  await input.fill('1 + 1');
  await input.press('Enter');

  const result = page.getByTestId('term-result');
  await expect(result).toBeVisible();
  // The mock returns `0x2` for any expression — assert that's what we render.
  await expect(result).toContainText('0x2');
});
