import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import type { Member, User } from '../src/api/types';
import { endpointFixture, endpointViewOf } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream, openLatestStream, pushStreamEvent, streamCount } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const BASE_PATH = `/e/${ENDPOINT_ID}`;
const ACCESS_PATH = `${BASE_PATH}/settings/access`;
const UNLOCK_PATH = `${BASE_PATH}/unlock`;
const PLACEHOLDER_PASSWORD = 'placeholder-pass-1';
const FAKE_GRANT = 'fake-grant-for-tests';
const WRONG_TEXT = 'That password did not work. Check it and try again.';
const OWNER: User = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21', email: 'owner@example.test', name: 'Olga Owner' };
const OPS: User = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a22', email: 'ops@example.test', name: 'Dev Ops' };
const OPS_MEMBER: Member = { user: OPS, role: 'viewer', added_at: '2026-11-21T10:00:00.000Z' };
const WIDTHS = [
  { width: 360, height: 740 },
  { width: 1024, height: 768 },
  { width: 1440, height: 900 },
] as const;

interface LockOptions {
  limited?: boolean;
}

function endpointWith(extra: Record<string, unknown> = {}): Record<string, unknown> {
  return { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}`, ...extra };
}

type PrepareRoutes = (page: Page) => Promise<void>;

async function openPage(page: Page, path: string, replies: ReplyMap = {}, prepare?: PrepareRoutes): Promise<void> {
  await mockApi(page, {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: endpointWith() },
    ...replies,
  });
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await installFakeStream(page);
  await prepare?.(page);
  await page.goto(path);
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

function isPatch(method: string, url: string): boolean {
  return method === 'PATCH' && url.endsWith(`/endpoints/${ENDPOINT_ID}`);
}

async function lockEndpoint(page: Page, options: LockOptions = {}): Promise<void> {
  await page.route(`**/api/v1/endpoints/${ENDPOINT_ID}`, (route) => {
    const request = route.request();
    if (request.method() !== 'GET' || request.headers()['x-webhooks-live-grant'] === FAKE_GRANT) {
      return route.fallback();
    }
    return route.fulfill({
      status: 401,
      contentType: 'application/json',
      body: JSON.stringify({ error: { code: 'grant_required', message: 'This endpoint needs a password.' } }),
    });
  });
  await page.route(`**/api/v1/endpoints/${ENDPOINT_ID}/unlock`, (route) => {
    if (options.limited === true) {
      return route.fulfill({
        status: 429,
        contentType: 'application/json',
        headers: { 'Retry-After': '30' },
        body: JSON.stringify({ error: { code: 'rate_limited', message: 'Too many attempts.' } }),
      });
    }
    const body: unknown = route.request().postDataJSON();
    const password = typeof body === 'object' && body !== null && 'password' in body ? body.password : null;
    if (password === PLACEHOLDER_PASSWORD) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ grant: FAKE_GRANT, expires_at: '2999-01-01T00:00:00.000Z' }),
      });
    }
    return route.fulfill({
      status: 401,
      contentType: 'application/json',
      body: JSON.stringify({ error: { code: 'wrong_password', message: 'The password is wrong.' } }),
    });
  });
}

for (const { width, height } of WIDTHS) {
  test(`the access section opens at ${width} px without sideways scrolling`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openPage(page, ACCESS_PATH);

    await expect(page.getByRole('heading', { level: 2, name: 'Access' })).toBeVisible();
    await page.getByRole('button', { name: 'Set a password' }).click();
    await page.getByRole('checkbox', { name: 'Accept captures only from these addresses' }).check();
    await page.getByRole('radio', { name: 'Basic (user name and password)' }).check();
    expect(await horizontalOverflow(page)).toBe(0);
  });
}

test('setting a password sends it once and then reports only that one is set', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const unlocks: string[] = [];
  page.on('request', (request) => {
    if (request.url().endsWith('/unlock')) {
      unlocks.push(request.url());
    }
  });
  await openPage(page, ACCESS_PATH, {
    [`PATCH /endpoints/${ENDPOINT_ID}`]: {
      body: endpointWith({
        view_protected: true,
        view_grant: { grant: FAKE_GRANT, expires_at: '2999-01-01T00:00:00.000Z' },
      }),
    },
  });
  await expect.poll(() => streamCount(page)).toBeGreaterThan(0);
  await openLatestStream(page);

  await page.getByRole('button', { name: 'Set a password' }).click();
  await page.getByLabel('New password').fill(PLACEHOLDER_PASSWORD);
  await page.getByLabel('Type it again').fill(`${PLACEHOLDER_PASSWORD}-typo`);
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('The two entries do not match.')).toBeVisible();

  await page.getByLabel('Type it again').fill(PLACEHOLDER_PASSWORD);
  const patched = page.waitForRequest((request) => isPatch(request.method(), request.url()));
  await page.getByRole('button', { name: 'Save' }).click();

  expect((await patched).postDataJSON()).toEqual({ view_password: PLACEHOLDER_PASSWORD });
  await expect(page.getByText('A password is set. It cannot be shown again.')).toBeVisible();
  await pushStreamEvent(page, 'endpoint_updated', endpointViewOf({ ...endpointFixture, id: ENDPOINT_ID }), '1');

  await expect(page.getByRole('heading', { level: 2, name: 'Access' })).toBeVisible();
  await expect(page.getByText('A password is set. It cannot be shown again.')).toBeVisible();
  await expect(page.getByLabel('New password')).toHaveCount(0);
  expect(unlocks).toEqual([]);
  expect(page.url()).not.toContain(PLACEHOLDER_PASSWORD);
});

test('the allow list rejects an invalid range and saves normalized entries', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openPage(page, ACCESS_PATH, {
    [`PATCH /endpoints/${ENDPOINT_ID}`]: {
      body: endpointWith({ sender_allow_cidrs: ['203.0.113.0/24', '198.51.100.9/32'] }),
    },
  });

  await page.getByRole('checkbox', { name: 'Accept captures only from these addresses' }).check();
  const input = page.getByRole('textbox', { name: 'Allowed addresses and ranges' });
  await input.fill('203.0.113.5/24');
  await input.press('Enter');
  await expect(page.getByText(/bits set after the prefix/)).toBeVisible();

  await input.fill('203.0.113.0/24');
  await input.press('Enter');
  await input.fill('198.51.100.9');
  await input.press('Enter');
  const patched = page.waitForRequest((request) => isPatch(request.method(), request.url()));
  await page.getByRole('button', { name: 'Save' }).click();

  expect((await patched).postDataJSON()).toEqual({ sender_allow_cidrs: ['203.0.113.0/24', '198.51.100.9/32'] });
});

test('a generated capture secret is shown once and sent with the method', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openPage(page, ACCESS_PATH, {
    [`PATCH /endpoints/${ENDPOINT_ID}`]: {
      body: endpointWith({ capture_auth: { kind: 'header', name: 'X-Test-Token' } }),
    },
  });

  await page.getByRole('radio', { name: 'Secret header' }).check();
  await page.getByLabel('Header name').fill('X-Test-Token');
  await page.getByRole('button', { name: 'Generate' }).click();
  const secret = page.getByLabel('Header value');
  await expect(secret).toBeFocused();
  await expect(page.getByText(/shown only once/)).toBeVisible();
  const value = await secret.inputValue();
  const patched = page.waitForRequest((request) => isPatch(request.method(), request.url()));
  await page.getByRole('button', { name: 'Save' }).click();

  expect((await patched).postDataJSON()).toEqual({ capture_auth: { kind: 'header', name: 'X-Test-Token', value } });
  await expect(page.getByText('The header X-Test-Token is required.')).toBeVisible();
  await expect(page.getByText(/shown only once/)).toHaveCount(0);
  await expect(secret).toHaveValue('');
});

test('a viewer sees the access settings read-only', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openPage(page, ACCESS_PATH, {
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: endpointWith({ my_role: 'viewer', sender_allow_cidrs: ['203.0.113.0/24'], view_protected: true }),
    },
  });

  await expect(page.getByText('Your role does not allow changes to access settings; ask the owner.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Change password' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Save' })).toHaveCount(0);
});

test('the owner lists members and the invite dialog explains an unknown email', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openPage(page, ACCESS_PATH, {
    'GET /instance': instanceWith({ auth_mode: 'sso', user: OWNER }),
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: endpointWith({ owner: OWNER, my_role: 'owner' }) },
    [`GET /endpoints/${ENDPOINT_ID}/members`]: { body: { items: [OPS_MEMBER] } },
    [`PUT /endpoints/${ENDPOINT_ID}/members`]: {
      status: 404,
      body: { error: { code: 'user_not_found', message: 'No user with this email has signed in.' } },
    },
  });

  const members = page.getByRole('list', { name: 'Members' });
  await expect(members.getByText('Dev Ops')).toBeVisible();
  await expect(members.getByText('Olga Owner (you)')).toBeVisible();
  await expect(page.getByText('Viewing password')).toHaveCount(0);

  await page.getByRole('button', { name: 'Invite' }).click();
  await page.getByLabel('Email address').fill('stranger@example.test');
  await page.getByRole('button', { name: 'Add member' }).click();
  await expect(page.getByText(/Nobody with this email has signed in yet/)).toBeVisible();
});

test('a locked endpoint sends the visitor to the unlock page and returns after the right password', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openPage(page, BASE_PATH, {}, (target) => lockEndpoint(target));

  await expect(page).toHaveURL(new RegExp(`${UNLOCK_PATH}\\?next=`));
  const password = page.getByLabel('Password');
  await expect(password).toHaveAttribute('autocomplete', 'current-password');
  await password.fill('wrong-guess');
  await page.getByRole('button', { name: 'Unlock' }).click();
  await expect(page.getByRole('alert')).toHaveText(WRONG_TEXT);
  await expect(password).toHaveValue('');
  await expect(password).toBeFocused();

  await password.fill(PLACEHOLDER_PASSWORD);
  await page.getByRole('button', { name: 'Unlock' }).click();
  await expect(page).toHaveURL(BASE_PATH);
  await expect(page.getByRole('heading', { name: 'This endpoint is locked' })).toHaveCount(0);
  expect(page.url()).not.toContain(PLACEHOLDER_PASSWORD);
});

test('too many unlock attempts disable the form and show the wait', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openPage(page, UNLOCK_PATH, {}, (target) => lockEndpoint(target, { limited: true }));

  await page.getByLabel('Password').fill('wrong-guess');
  await page.getByRole('button', { name: 'Unlock' }).click();

  await expect(page.getByText('Too many attempts. Wait before trying again.')).toBeVisible();
  await expect(page.getByText(/You can try again in \d+ s\./)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Unlock' })).toBeDisabled();
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the access section has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openPage(page, ACCESS_PATH, {
      [`GET /endpoints/${ENDPOINT_ID}`]: { body: endpointWith({ view_protected: true }) },
    });
    await chooseTheme(page, theme);
    await page.getByRole('button', { name: 'Change password' }).click();
    await page.getByRole('checkbox', { name: 'Accept captures only from these addresses' }).check();
    await page.getByRole('radio', { name: 'Basic (user name and password)' }).check();
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByRole('heading', { level: 2, name: 'Access' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the members list has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openPage(page, ACCESS_PATH, {
      'GET /instance': instanceWith({ auth_mode: 'sso', user: OWNER }),
      [`GET /endpoints/${ENDPOINT_ID}`]: { body: endpointWith({ owner: OWNER, my_role: 'owner' }) },
      [`GET /endpoints/${ENDPOINT_ID}/members`]: { body: { items: [OPS_MEMBER] } },
    });
    await chooseTheme(page, theme);
    await expect(page.getByRole('list', { name: 'Members' }).getByText('Dev Ops')).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the unlock page has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openPage(page, UNLOCK_PATH, {}, (target) => lockEndpoint(target));
    await chooseTheme(page, theme);
    await page.getByLabel('Password').fill('wrong-guess');
    await page.getByRole('button', { name: 'Unlock' }).click();
    await expect(page.getByRole('alert')).toHaveText(WRONG_TEXT);

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
