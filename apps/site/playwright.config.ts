import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  use: { baseURL: process.env.SITE_TEST_URL ?? 'http://127.0.0.1:4174', trace: 'retain-on-failure' },
  projects: [
    { name: 'desktop', use: { viewport: { width: 1440, height: 1000 } } },
    { name: 'mobile', use: { ...devices['iPhone 13'], defaultBrowserType: 'chromium' } }
  ],
  webServer: process.env.SITE_TEST_URL ? undefined : {
    command: 'node scripts/serve-build.mjs 4174',
    url: 'http://127.0.0.1:4174',
    reuseExistingServer: !process.env.CI
  }
});
