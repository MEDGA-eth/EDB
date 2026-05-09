import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'e2e',
  fullyParallel: false,
  retries: 1,
  use: {
    baseURL: 'http://127.0.0.1:5173',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command: 'bun run dev --mode=mock',
    url: 'http://127.0.0.1:5173',
    // Always start a fresh server. Reusing a stale dev server (especially
    // one with cached, pre-audit fixture data) was the silent failure
    // mode that motivated this audit, so we eat the boot cost every time.
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
