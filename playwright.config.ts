import { defineConfig, devices } from '@playwright/test';

// Interface tests: the real screens, in a browser, against the mock API (the
// same page `npm run dev` serves). They play the part TestFX played in the
// Java app. The real app is checked separately, with a few smoke tests.
export default defineConfig({
  testDir: 'e2e',
  reporter: 'line',
  use: { baseURL: 'http://localhost:5180' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 820 } } }],
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5180',
    reuseExistingServer: true,
  },
});
