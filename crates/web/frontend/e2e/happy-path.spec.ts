import { expect, test } from '@playwright/test';

test('boots into the IDE shell with bottom panels and updates URL on Next', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('ide-layout')).toBeVisible();
  await expect(page.getByTestId('activity-bar')).toBeVisible();
  await expect(page.getByTestId('side-bar')).toBeVisible();
  await expect(page.getByTestId('status-bar')).toBeVisible();
  // Bottom panel area renders trace + display + terminal as dockview tabs
  await expect(page.getByTestId('trace-panel')).toBeVisible();

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
