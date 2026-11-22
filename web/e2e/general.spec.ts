import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture, endpointViewOf } from '../src/test/fixtures/endpoint';
import { EXAMPLE_SECRET_NAME, usageFixture } from '../src/test/fixtures/general';
import { instanceFixture, PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream, openLatestStream, pushStreamEvent, streamCount } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type Reply, type ReplyMap } from './support/mockApi';

const BASE_PATH = `/e/${ENDPOINT_ID}`;
const GENERAL_PATH = `${BASE_PATH}/settings/general`;
const CLONE_ID = '0123456789abcdefghjkmnpr';
const WEEK_SECONDS = 604800;
const MONTH_SECONDS = 2592000;
const OWNER = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21', email: 'owner@example.test', name: 'Owner' };

function ownerEndpoint(): Record<string, unknown> {
  return {
    ...endpointFixture,
    id: ENDPOINT_ID,
    url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
    my_role: 'owner',
    owner: OWNER,
    max_captures: 500,
    retention: { mode: 'sliding', seconds: null },
    capture_ttl_seconds: null,
    sender_allow_cidrs: [],
    capture_auth: null,
    verifier: null,
    view_protected: false,
  };
}

function signedInInstance(): Reply {
  return instanceWith({
    auth_mode: 'mixed',
    user: OWNER,
    limits: {
      ...instanceFixture.limits,
      retention_seconds: WEEK_SECONDS,
      max_retention_seconds: MONTH_SECONDS,
      never_expire_endpoints: 3,
      max_captures_settable: 500,
    },
  });
}

function replies(extra: ReplyMap = {}): ReplyMap {
  return {
    'GET /instance': signedInInstance(),
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: ownerEndpoint() },
    [`GET /endpoints/${ENDPOINT_ID}/aliases`]: { body: { items: [] } },
    'GET /folders': { body: { items: [] } },
    'GET /usage': { body: usageFixture },
    ...extra,
  };
}

async function openGeneral(page: Page, extra: ReplyMap = {}): Promise<void> {
  await mockApi(page, replies(extra));
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await installFakeStream(page);
  await page.goto(GENERAL_PATH);
  await expect(page.getByRole('heading', { level: 2, name: 'General' })).toBeVisible();
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

test('the lifetime text follows the chosen mode and disables the window when it does not apply', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openGeneral(page);

  await expect(
    page.getByText('The endpoint and its captures are deleted after 7 days without activity; senders then get 410.'),
  ).toBeVisible();

  await page.getByRole('radio', { name: 'Keep until deleted' }).check();

  await expect(page.getByText('The endpoint and its captures are kept until you delete them.')).toBeVisible();
  await expect(page.getByRole('combobox', { name: 'Inactivity window' })).toBeDisabled();
});

test('an alias is checked while it is typed and then added', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openGeneral(page, {
    'GET /aliases/example-hook/availability': {
      body: { alias: 'example-hook', available: true, reason: null },
    },
    [`PUT /endpoints/${ENDPOINT_ID}/aliases/example-hook`]: {
      body: { alias: 'example-hook', url: `${PUBLIC_URL}/a/example-hook`, created_at: '2026-11-21T10:00:00.000Z' },
    },
  });

  await page.getByRole('textbox', { name: 'New alias' }).fill('example-hook');
  await expect(page.getByText('Available.')).toBeVisible();
  await page.getByRole('button', { name: 'Add alias' }).click();

  await expect(page.getByText('Alias added.')).toBeVisible();
});

test('a taken alias cannot be added', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openGeneral(page, {
    'GET /aliases/taken-hook/availability': {
      body: { alias: 'taken-hook', available: false, reason: 'taken' },
    },
  });

  await page.getByRole('textbox', { name: 'New alias' }).fill('taken-hook');

  await expect(page.getByText('This alias is already in use.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add alias' })).toBeDisabled();
});

test('the usage meters show the account meters from the usage route', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openGeneral(page);

  await expect(page.getByRole('progressbar', { name: 'Endpoints' })).toHaveAttribute('aria-valuetext', '7 of 20');
  await expect(page.getByRole('progressbar', { name: 'Captures' })).toHaveAttribute('aria-valuetext', '0 of 500');
});

test('a clone lists the secrets that were not copied and then opens the new endpoint', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openGeneral(page, {
    [`POST /endpoints/${ENDPOINT_ID}/clone`]: {
      status: 201,
      body: {
        ...ownerEndpoint(),
        id: CLONE_ID,
        url: `${PUBLIC_URL}/${CLONE_ID}`,
        title: 'stripe-dev copy',
        secrets_missing: [EXAMPLE_SECRET_NAME],
        skipped: [],
      },
    },
  });

  await page.getByRole('button', { name: 'Clone endpoint' }).click();
  const dialog = page.getByRole('dialog', { name: 'Clone endpoint' });
  const cloned = page.waitForRequest((request) => request.method() === 'POST' && request.url().endsWith('/clone'));
  await dialog.getByRole('button', { name: 'Clone', exact: true }).click();

  expect((await cloned).postDataJSON()).toEqual({
    title: 'stripe test copy',
    include: ['response', 'forward_rules', 'rules', 'channels', 'schedules'],
  });

  await expect(dialog.getByText('Secrets were not copied')).toBeVisible();
  await expect(dialog.getByText(EXAMPLE_SECRET_NAME)).toBeVisible();
  await dialog.getByRole('button', { name: 'Open the new endpoint' }).click();

  await expect(page).toHaveURL(new RegExp(`/e/${CLONE_ID}/settings/general$`));
});

test('saving the title keeps the form while the server announces the saved endpoint without its identity fields', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const saved = { ...ownerEndpoint(), title: 'Renamed', tags: ['billing'] };
  await openGeneral(page, {
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: { ...ownerEndpoint(), tags: ['billing'] } },
    [`PATCH /endpoints/${ENDPOINT_ID}`]: { body: saved },
  });
  await expect.poll(() => streamCount(page)).toBeGreaterThan(0);
  await openLatestStream(page);

  await page.getByLabel('Title', { exact: true }).fill('Renamed');
  const patched = page.waitForRequest((request) => request.method() === 'PATCH');
  await page.getByRole('button', { name: 'Save' }).click();
  await patched;
  const announced = endpointViewOf({
    ...endpointFixture,
    id: ENDPOINT_ID,
    url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
    title: 'Renamed',
  });
  await pushStreamEvent(page, 'endpoint_updated', announced, '1');

  await expect(page.getByLabel('Title', { exact: true })).toHaveValue('Renamed');
  await expect(page.getByText('This page could not be loaded')).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 2, name: 'General' })).toBeVisible();
});

test('the clone dialog closes with Escape and gives focus back to its button', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openGeneral(page);

  const trigger = page.getByRole('button', { name: 'Clone endpoint' });
  await trigger.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Clone endpoint' })).toBeVisible();

  await page.keyboard.press('Escape');

  await expect(page.getByRole('dialog', { name: 'Clone endpoint' })).toBeHidden();
  await expect(trigger).toBeFocused();
});

test('the general section fits 360 px without sideways scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await openGeneral(page);

  await expect.poll(() => horizontalOverflow(page)).toBe(0);
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the lifetime, alias, usage and clone parts have no axe violations in the ${theme.toLowerCase()} theme`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openGeneral(page);
    await chooseTheme(page, theme);
    await expect(page.getByRole('heading', { level: 2, name: 'General' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
