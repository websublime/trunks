import { defineConfig, devices } from '@playwright/test';

// Smoke tests of the console against the real `trunks` binary with the build embedded
// (`cargo xtask build-dashboard && cargo build -p trunks`). Each spec starts its own server
// on a free port with a throw-away database.
export default defineConfig({
  testDir: 'tests',
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env['CI'],
  retries: 0,
  reporter: process.env['CI'] ? [['list'], ['github']] : 'list',
  timeout: 60_000,
  use: { trace: 'retain-on-failure', colorScheme: 'dark' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
