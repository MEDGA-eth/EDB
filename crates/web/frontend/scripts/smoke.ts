import { chromium, type Page } from 'playwright';

const url = process.env.URL ?? 'http://127.0.0.1:3000';
const out = process.env.OUT ?? '../../../resources/edb-web.png';
const dark = process.env.DARK === '1';

async function expectVisible(page: Page, testid: string, label: string) {
  await page.waitForSelector(`[data-testid="${testid}"]`, { state: 'visible', timeout: 15_000 });
  console.log(`  ✓ ${label}`);
}

const browser = await chromium.launch();
const ctx = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
});
const page = await ctx.newPage();

const consoleErrors: string[] = [];
page.on('console', (msg) => {
  if (msg.type() === 'error') consoleErrors.push(msg.text());
});
page.on('pageerror', (err) => consoleErrors.push(`pageerror: ${err.message}`));

if (dark) await page.addInitScript(() => localStorage.setItem('edb-web:theme', 'dark'));
await page.goto(url, { waitUntil: 'networkidle' });

console.log('1. Boot');
await expectVisible(page, 'ide-layout', 'IDE shell');
await expectVisible(page, 'activity-bar', 'Activity bar');
await expectVisible(page, 'status-bar', 'Status bar');

console.log('2. File explorer renders addresses from real trace');
await expectVisible(page, 'side-bar', 'Sidebar');
await page.waitForTimeout(800);
const treeitems = await page.locator('[role="treeitem"]').count();
if (treeitems === 0) throw new Error('explorer has no treeitems');
console.log(`  ✓ ${treeitems} treeitem(s)`);

console.log('3. Trace sidebar shows entries from real trace');
// Trace lives in the sidebar now; click the activity-bar trace icon to show it.
await page.locator('[data-testid="activity-trace"]').click();
await page.waitForTimeout(500);
const traceEntries = await page.locator('[data-testid^="trace-entry-"]').count();
if (traceEntries === 0) throw new Error('no trace entries');
console.log(`  ✓ ${traceEntries} trace entries`);
// Switch back to explorer for the screenshot
await page.locator('[data-testid="activity-explorer"]').click();
await page.waitForTimeout(300);

console.log('4. Snapshot navigation via URL hash');
await page.evaluate(() => {
  location.hash = '50';
  window.dispatchEvent(new Event('hashchange'));
});
await page.waitForTimeout(800);
const finalHash = await page.evaluate(() => location.hash);
if (!finalHash.includes('50')) throw new Error(`expected #50, got ${finalHash}`);
console.log(`  ✓ URL hash = ${finalHash}`);

console.log('5. Display panel Variables tab');
const dispTab = page.locator('.dv-tab').filter({ hasText: /^Display$/ }).first();
await dispTab.click();
await page.waitForTimeout(300);
const varsTab = page.locator('[data-testid="display-tab-vars"]');
if ((await varsTab.count()) > 0) {
  await varsTab.click();
  console.log('  ✓ Variables tab clicked');
}

console.log('6. Capture screenshot');
await page.waitForTimeout(400);
await page.screenshot({ path: out });
console.log(`  ✓ wrote ${out}`);

if (consoleErrors.length > 0) {
  console.error('\nBrowser console errors:');
  for (const e of consoleErrors) console.error('  -', e);
  throw new Error(`smoke saw ${consoleErrors.length} console error(s)`);
}

await browser.close();
console.log('SMOKE OK');
