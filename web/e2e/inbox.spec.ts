import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import type { Capture, CaptureSummary } from '../src/api/types';
import {
  captureIdOf,
  inboxCapture,
  PAYMENTS_ENDPOINT_ID,
  paymentsEndpoint,
  SHIPPING_ENDPOINT_ID,
  shippingEndpoint,
} from '../src/test/fixtures/inbox';
import {
  failLatestStream,
  installFakeStream,
  latestStreamUrl,
  openLatestStream,
  pushStreamEvent,
  streamCount,
} from './support/fakeStream';
import { apiError, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const CHOSEN_PATH = `/inbox?scope=endpoints:${PAYMENTS_ENDPOINT_ID},${SHIPPING_ENDPOINT_ID}`;
const LIST_NAME = 'Captures';
const GONE_TITLE = 'An endpoint in this scope is no longer available';
const CAPTURE_HEADING = `Capture ${captureIdOf(1).slice(-8)}`;

function endpointReplies(): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${PAYMENTS_ENDPOINT_ID}`]: { body: paymentsEndpoint },
    [`GET /endpoints/${SHIPPING_ENDPOINT_ID}`]: { body: shippingEndpoint },
  };
}

async function serveInbox(page: Page, server: CaptureSummary[]): Promise<void> {
  await page.route('**/api/v1/inbox/captures**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: server, next_cursor: null }),
    }),
  );
}

function initialCaptures(): CaptureSummary[] {
  return [
    inboxCapture(PAYMENTS_ENDPOINT_ID, 1, '/hooks/payments'),
    inboxCapture(SHIPPING_ENDPOINT_ID, 2, '/hooks/shipping'),
  ];
}

function rowsOf(page: Page) {
  return page.getByRole('listbox', { name: LIST_NAME }).getByRole('option');
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test('arrivals from two endpoints merge once after a forced reconnect', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeStream(page);
  await mockApi(page, endpointReplies());
  const server = initialCaptures();
  await serveInbox(page, server);
  await page.goto(CHOSEN_PATH);
  const rows = rowsOf(page);
  await expect(rows).toHaveCount(2);
  await expect.poll(() => streamCount(page)).toBeGreaterThan(0);
  const opened = await streamCount(page);
  await openLatestStream(page);

  const arrival = inboxCapture(SHIPPING_ENDPOINT_ID, 4, '/hooks/shipping/late');
  server.unshift(arrival);
  await pushStreamEvent(page, 'capture', arrival, 'vector-1');
  await expect(rows).toHaveCount(3);

  await failLatestStream(page);
  await expect.poll(() => streamCount(page)).toBe(opened + 1);
  await expect.poll(() => latestStreamUrl(page)).toContain('last_event_id=vector-1');
  await openLatestStream(page);

  const newer = inboxCapture(PAYMENTS_ENDPOINT_ID, 5, '/hooks/payments/new');
  server.unshift(newer);
  await pushStreamEvent(page, 'capture', arrival, 'vector-1');
  await pushStreamEvent(page, 'capture', newer, 'vector-2');
  await expect(rows).toHaveCount(4);
});

test('a gone endpoint in the scope asks the user to choose the scope again', async ({ page }) => {
  await installFakeStream(page);
  await mockApi(page, {
    ...endpointReplies(),
    'GET /inbox/captures': apiError(410, 'endpoint_gone', 'One of the endpoints is gone'),
  });
  await page.goto(CHOSEN_PATH);
  await expect(page.getByRole('alert').filter({ hasText: GONE_TITLE })).toBeVisible();
  await page.getByRole('button', { name: 'Choose scope' }).click();
  await expect(page.getByRole('combobox', { name: 'Scope' })).toBeFocused();
});

test('a deep link opens the capture detail next to the merged list', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeStream(page);
  const summary = inboxCapture(PAYMENTS_ENDPOINT_ID, 1, '/hooks/payments');
  const capture: Capture = {
    ...summary,
    url: `https://webhooks.example.test/${PAYMENTS_ENDPOINT_ID}/hooks/payments`,
    host: 'webhooks.example.test',
    headers: [],
    query: [],
    form: null,
    body: { encoding: 'utf8', data: '{"ok":true}', truncated: false },
    forwards: [],
  };
  await mockApi(page, {
    ...endpointReplies(),
    [`GET /endpoints/${PAYMENTS_ENDPOINT_ID}/captures/${summary.id}`]: { body: capture },
  });
  await serveInbox(page, [summary]);
  await page.goto(`/inbox/${PAYMENTS_ENDPOINT_ID}/${summary.id}?scope=endpoints:${PAYMENTS_ENDPOINT_ID}`);
  await expect(page.getByRole('heading', { name: CAPTURE_HEADING, exact: true })).toBeVisible();
  await expect(rowsOf(page)).toHaveCount(1);
});

test('the combined inbox fits a 360 px viewport without horizontal scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await installFakeStream(page);
  await mockApi(page, endpointReplies());
  await serveInbox(page, initialCaptures());
  await page.goto(CHOSEN_PATH);
  await expect(rowsOf(page)).toHaveCount(2);
  const width = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(width).toBeLessThanOrEqual(360);
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the combined inbox has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await installFakeStream(page);
    await mockApi(page, endpointReplies());
    await serveInbox(page, initialCaptures());
    await page.goto(CHOSEN_PATH);
    await expect(rowsOf(page)).toHaveCount(2);
    await chooseTheme(page, theme);

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
