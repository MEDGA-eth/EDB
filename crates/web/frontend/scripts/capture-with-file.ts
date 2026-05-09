// Capture EDB Web UI screenshots with a real Solidity file open in the
// editor (so the README screenshots actually showcase the syntax-highlighted
// code view, not just the empty-state placeholder).
import { chromium, type Page } from 'playwright';

const url = process.env.URL ?? 'http://127.0.0.1:5173';
const out = process.env.OUT ?? '../../../resources/edb-web.png';
const dark = process.env.DARK === '1';
// Optional file picker: which `.sol` file to open. Defaults to the first
// available file in the explorer.
const fileMatch = process.env.FILE ?? '';

const browser = await chromium.launch();
const ctx = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
});
const page = await ctx.newPage();

if (dark) {
  await page.addInitScript(() => localStorage.setItem('edb-web:theme', 'dark'));
}

await page.goto(url, { waitUntil: 'networkidle' });
await page.waitForTimeout(800);

// Find a Solidity file in the explorer. The explorer renders treeitems with a
// data-testid prefixed `explorer-file-`; we click the first match (or one
// whose label contains FILE).
async function clickFile(page: Page) {
  const files = page.locator('[data-testid^="explorer-file-"]');
  const count = await files.count();
  if (count === 0) throw new Error('no files in explorer — engine may not be ready');
  let target = files.first();
  if (fileMatch) {
    const matched = files.filter({ hasText: fileMatch });
    if ((await matched.count()) > 0) target = matched.first();
  }
  await target.click();
}

await clickFile(page);

// Wait for the editor to actually mount + render Solidity syntax.
await page.waitForSelector('.cm-editor', { state: 'visible', timeout: 10_000 });
// Give CodeMirror a beat to highlight + paint.
await page.waitForTimeout(900);

await page.screenshot({ path: out, fullPage: false });
await browser.close();
console.log(`wrote ${out}`);
