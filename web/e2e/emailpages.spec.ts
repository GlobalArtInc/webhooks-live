import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import { DISABLE_TOKEN, pendingAddressFixture, VERIFY_TOKEN } from '../src/test/fixtures/notifications';
import { apiError, mockApi } from './support/mockApi';

const VERIFY_PAGE = '/email/verify';
const DISABLE_PAGE = '/notifications/disable';
const VERIFY_BUTTON = 'Confirm this address';
const DISABLE_BUTTON = 'Stop these notifications';

for (const colorScheme of ['light', 'dark'] as const) {
  test(`the e-mail confirmation page has no serious accessibility issues in ${colorScheme} theme`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await mockApi(page, {});
    await page.goto(`${VERIFY_PAGE}?token=${VERIFY_TOKEN}`);
    await expect(page.getByRole('button', { name: VERIFY_BUTTON })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    const serious = results.violations.filter(
      (violation) => violation.impact === 'serious' || violation.impact === 'critical',
    );
    expect(serious).toEqual([]);
  });
}

test('the confirmation link leaves the address bar and only a button press confirms the address', async ({ page }) => {
  const posts: string[] = [];
  page.on('request', (request) => {
    if (request.method() === 'POST') {
      posts.push(request.url());
    }
  });
  await mockApi(page, {
    'POST /email-addresses/verify': {
      body: { ...pendingAddressFixture, verified: true },
    },
  });

  await page.goto(`${VERIFY_PAGE}?token=${VERIFY_TOKEN}`);
  await expect(page.getByRole('button', { name: VERIFY_BUTTON })).toBeVisible();
  await expect.poll(() => new URL(page.url()).search).toBe('');
  expect(posts).toEqual([]);

  await page.getByRole('button', { name: VERIFY_BUTTON }).click();

  await expect(page.getByRole('status')).toContainText(`${pendingAddressFixture.address} is confirmed.`);
  expect(posts.filter((url) => url.endsWith('/api/v1/email-addresses/verify'))).toHaveLength(1);
});

test('a refused confirmation link says so and offers no retry', async ({ page }) => {
  await mockApi(page, {
    'POST /email-addresses/verify': apiError(422, 'invalid_token', 'The link is invalid or has expired.'),
  });

  await page.goto(`${VERIFY_PAGE}?token=${VERIFY_TOKEN}`);
  await page.getByRole('button', { name: VERIFY_BUTTON }).click();

  await expect(page.getByRole('alert')).toContainText('This link is invalid or has expired.');
  await expect(page.getByRole('button', { name: VERIFY_BUTTON })).toHaveCount(0);
});

for (const colorScheme of ['light', 'dark'] as const) {
  test(`the disable page has no serious accessibility issues in ${colorScheme} theme`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await mockApi(page, {});
    await page.goto(`${DISABLE_PAGE}?token=${DISABLE_TOKEN}`);
    await expect(page.getByRole('button', { name: DISABLE_BUTTON })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    const serious = results.violations.filter(
      (violation) => violation.impact === 'serious' || violation.impact === 'critical',
    );
    expect(serious).toEqual([]);
  });
}

test('the disable link stops the notifications after the button press', async ({ page }) => {
  await mockApi(page, {});
  await page.route('**/api/v1/notifications/disable', (route) => route.fulfill({ status: 204 }));

  await page.goto(`${DISABLE_PAGE}?token=${DISABLE_TOKEN}`);
  await expect.poll(() => new URL(page.url()).search).toBe('');
  await page.getByRole('button', { name: DISABLE_BUTTON }).click();

  await expect(page.getByRole('status')).toContainText('These notifications are stopped.');
});
