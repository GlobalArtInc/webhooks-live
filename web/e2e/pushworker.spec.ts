import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { instanceFixture, PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi } from './support/mockApi';

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/notifications`;
const SIGNED_IN_USER = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4f01', email: 'alex@example.test', name: 'Alex' };
const FEATURES = { ...instanceFixture.features, notifications: ['webhook', 'push'] };
const PUBLIC_KEY = 'BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM';
const STUB_ENDPOINT = 'https://fcm.googleapis.com/fcm/send/e2e-stub';

test.use({ permissions: ['notifications'] });

async function stubPushManager(page: Page): Promise<void> {
  await page.addInitScript((endpoint) => {
    PushManager.prototype.getSubscription = () => Promise.resolve(null);
    PushManager.prototype.subscribe = () =>
      Promise.resolve({
        endpoint,
        toJSON: () => ({ endpoint, keys: { p256dh: 'p256dh-stub', auth: 'auth-stub' } }),
      } as unknown as PushSubscription);
  }, STUB_ENDPOINT);
}

async function openPushEditor(page: Page, posted: string[]): Promise<void> {
  await mockApi(page, {
    'GET /instance': instanceWith({ features: FEATURES, user: SIGNED_IN_USER }),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}`, my_role: 'owner' },
    },
    [`GET /endpoints/${ENDPOINT_ID}/notifications`]: { body: { items: [] } },
    'GET /endpoints': { body: { items: [], next_cursor: null } },
    'GET /folders': { body: { items: [] } },
    'GET /email-addresses': { body: { items: [] } },
    [`GET /endpoints/${ENDPOINT_ID}/secrets`]: { body: { items: [] } },
    'GET /push/public-key': { body: { public_key: PUBLIC_KEY } },
  });
  await page.route('**/api/v1/push/subscriptions', async (route) => {
    posted.push(route.request().postData() ?? '');
    await route.fulfill({
      status: 201,
      contentType: 'application/json',
      body: JSON.stringify({ id: 'sub-1', created_at: '2026-11-22T10:00:00.000Z' }),
    });
  });
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await installFakeStream(page);
  await page.goto(SECTION_PATH);
  await page.getByRole('button', { name: 'Add channel' }).click();
  await page.getByLabel('Kind').selectOption('push');
}

async function registrationScripts(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    const registrations = await navigator.serviceWorker.getRegistrations();
    return registrations.map((registration) => `${registration.scope}|${registration.active?.scriptURL ?? ''}`);
  });
}

test('the push worker is registered only after the user starts enabling push, with no console errors', async ({
  page,
}) => {
  const problems: string[] = [];
  const posted: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') {
      problems.push(message.text());
    }
  });
  page.on('pageerror', (error) => problems.push(error.message));
  await stubPushManager(page);
  await openPushEditor(page, posted);

  const allow = page.getByRole('button', { name: 'Allow in this browser' });
  await expect(allow).toBeVisible();
  expect(await registrationScripts(page)).toEqual([]);

  await allow.click();

  await expect(page.getByRole('status').filter({ hasText: 'This browser is subscribed' })).toBeVisible();
  await expect
    .poll(() => registrationScripts(page))
    .toEqual([`${new URL(page.url()).origin}/|${new URL(page.url()).origin}/sw.js`]);
  expect(posted).toHaveLength(1);
  expect(JSON.parse(posted[0] ?? '{}')).toMatchObject({
    endpoint: STUB_ENDPOINT,
    keys: { p256dh: 'p256dh-stub', auth: 'auth-stub' },
  });
  expect(problems).toEqual([]);
});
