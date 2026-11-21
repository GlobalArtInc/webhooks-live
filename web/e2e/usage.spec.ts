import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import type { Usage } from '../src/api/usage';
import {
  SIGNED_IN_USER,
  fullOutboundMeter,
  meterFixture,
  nearStorageMeter,
  usageFixture,
} from '../src/test/fixtures/usage';
import { instanceWith, mockApi } from './support/mockApi';

const USAGE_PATH = '/usage';
const USAGE_REQUEST = '/api/v1/usage';
const WIDTHS = [
  { width: 360, height: 740 },
  { width: 1440, height: 900 },
] as const;

async function openUsage(
  page: Page,
  usage: Usage = usageFixture([nearStorageMeter(), fullOutboundMeter(), meterFixture()]),
): Promise<void> {
  await mockApi(page, {
    'GET /instance': instanceWith({ auth_mode: 'mixed', user: SIGNED_IN_USER }),
    'GET /usage': { body: usage },
  });
  await page.goto(USAGE_PATH);
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

test('shows the at-limit sentence for a meter at 90 percent and for one at 100 percent', async ({ page }) => {
  await openUsage(page);

  await expect(page.getByRole('heading', { level: 1, name: 'Usage and limits' })).toBeVisible();
  await expect(page.getByText('Tier member')).toBeVisible();
  await expect(page.getByText('90% used. Close to the limit.')).toBeVisible();
  await expect(page.getByText(/^At the limit\. New items are refused until it resets in /)).toBeVisible();
  await expect(page.getByRole('progressbar', { name: 'Outbound requests' })).toHaveAttribute(
    'aria-valuetext',
    '200 of 200',
  );
});

test('the header warns when a meter is near its limit and the link opens the page', async ({ page }) => {
  await mockApi(page, {
    'GET /instance': instanceWith({ auth_mode: 'mixed', user: SIGNED_IN_USER }),
    'GET /usage': { body: usageFixture([nearStorageMeter()]) },
  });
  await page.goto('/settings');

  await page.getByRole('link', { name: /Storage: 90% of its limit/ }).click();

  await expect(page).toHaveURL(USAGE_PATH);
  await expect(page.getByRole('heading', { level: 1, name: 'Usage and limits' })).toBeVisible();
});

test('shows the not-available page when the instance has no sign-in', async ({ page }) => {
  await mockApi(page, { 'GET /instance': instanceWith() });
  await page.goto(USAGE_PATH);

  await expect(page.getByRole('heading', { level: 1, name: 'Usage and limits are not available here' })).toBeVisible();
});

test('refetches the counters when the window regains focus', async ({ page }) => {
  await openUsage(page);
  await expect(page.getByRole('progressbar', { name: 'Outbound requests' })).toBeVisible();

  let requests = 0;
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === USAGE_REQUEST) {
      requests += 1;
    }
  });
  await page.evaluate(() => window.dispatchEvent(new Event('visibilitychange')));

  await expect.poll(() => requests).toBeGreaterThan(0);
});

for (const { width, height } of WIDTHS) {
  test(`the page and the header warning fit at ${width} px without sideways scrolling`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openUsage(page);

    await expect(page.getByRole('heading', { level: 1, name: 'Usage and limits' })).toBeVisible();
    await expect(page.getByRole('link', { name: /Outbound requests: limit reached/ })).toBeVisible();
    await expect(page.getByText(/^At the limit\. New items are refused until it resets in /)).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);
  });
}

for (const theme of ['Light', 'Dark'] as const) {
  test(`the usage page has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openUsage(page);
    await chooseTheme(page, theme);
    await expect(page.getByText(/^At the limit\. New items are refused until it resets in /)).toBeVisible();

    const result = await new AxeBuilder({ page }).analyze();
    expect(result.violations).toEqual([]);
  });
}
