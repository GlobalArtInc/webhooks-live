import { defineConfig, devices } from '@playwright/test';

const DEV_URL = 'http://127.0.0.1:5173';
const PREVIEW_URL = 'http://127.0.0.1:4173';
const CSP_SPEC = /csp\.spec\.ts/;

export default defineConfig({
  testDir: './e2e',
  use: {
    baseURL: DEV_URL,
    reducedMotion: 'reduce',
  },
  projects: [
    {
      name: 'chromium',
      testIgnore: CSP_SPEC,
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'csp',
      testMatch: CSP_SPEC,
      use: { ...devices['Desktop Chrome'], baseURL: PREVIEW_URL },
    },
  ],
  webServer: [
    {
      command: 'pnpm exec vite --host 127.0.0.1 --port 5173 --strictPort',
      url: DEV_URL,
      reuseExistingServer: true,
    },
    {
      command: 'pnpm exec vite build && pnpm exec vite preview --host 127.0.0.1 --port 4173 --strictPort',
      url: PREVIEW_URL,
      reuseExistingServer: true,
      timeout: 180_000,
    },
  ],
});
