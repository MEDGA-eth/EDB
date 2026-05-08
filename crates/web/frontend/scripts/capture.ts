import { chromium } from 'playwright';

const url = process.env.URL ?? 'http://127.0.0.1:5173';
const out = process.env.OUT ?? '../../../resources/edb-web.png';
const dark = process.env.DARK === '1';

const browser = await chromium.launch();
const context = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
});
const page = await context.newPage();
if (dark) {
  await page.addInitScript(() => {
    localStorage.setItem('edb-web:theme', 'dark');
  });
}
await page.goto(url, { waitUntil: 'networkidle' });
await page.waitForTimeout(800); // settle
await page.screenshot({ path: out, fullPage: false });
await browser.close();
console.log(`wrote ${out}`);
