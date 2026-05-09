import { expect, test } from '@playwright/test';

// Exercises a flow that only makes sense once the mock dataset is realistic:
// the command palette's snapshot prefix (`:`) only renders rows up to the
// snapshot row cap (50), so we navigate to snapshot 25, then run the
// `next call` command — which advances by 5 in the mock (matching the real
// engine's `edb_getNextCall(25)` behaviour for this tx) → snapshot 30.
test('command palette navigates between snapshots', async ({ page }) => {
  await page.goto('/');

  // Wait for the IDE shell to be up — both the status bar (with snapshot
  // label) and the trace panel must be mounted before we drive the palette.
  // Trace now lives in the sidebar; activate it so the panel mounts.
  await expect(page.getByTestId('status-bar')).toBeVisible();
  await page.getByTestId('activity-trace').click();
  await expect(page.getByTestId('trace-panel')).toBeVisible();
  await expect(page.getByTestId('snapshot-label')).toContainText('/ 196');

  // Open the command palette (Cmd+P / Ctrl+P).
  // Use the platform-appropriate modifier so this works on macOS CI runners.
  const isMac = process.platform === 'darwin';
  await page.keyboard.press(isMac ? 'Meta+P' : 'Control+P');
  await expect(page.getByTestId('command-palette')).toBeVisible();

  // Type `:25` to enter snapshot-jump mode and click the matching row
  // explicitly. Pressing Enter would activate the first ranked row, which
  // is brittle when the rank function may surface "snap:5" ahead of "snap:25".
  const palette = page.getByTestId('palette-input');
  await palette.fill(':25');
  const snap25Row = page.getByTestId('palette-row-snap:25');
  await expect(snap25Row).toBeVisible();
  await snap25Row.click();

  await expect(page.getByTestId('command-palette')).toBeHidden();
  await expect(page).toHaveURL(/#25$/);
  // Status bar shows snapshots 1-based now — internal id 25 → label 26.
  // URL hash and palette `:N` syntax remain 0-indexed (those are addresses,
  // not display labels).
  await expect(page.getByTestId('snapshot-label')).toContainText('snapshot 26');

  // Re-open the palette and run the `next call` command. The mock advances
  // by 5 (matching the real engine's getNextCall behaviour for this tx),
  // so snapshot 25 → 30 (display: 31).
  await page.keyboard.press(isMac ? 'Meta+P' : 'Control+P');
  await expect(page.getByTestId('command-palette')).toBeVisible();
  const palette2 = page.getByTestId('palette-input');
  await palette2.fill('>next call');
  const nextCallRow = page.getByTestId('palette-row-cmd:nav.next-call');
  await expect(nextCallRow).toBeVisible();
  await nextCallRow.click();

  await expect(page.getByTestId('command-palette')).toBeHidden();
  await expect(page.getByTestId('snapshot-label')).toContainText('snapshot 31');
  await expect(page).toHaveURL(/#30$/);
});
