import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page, type Route } from '@playwright/test';
import type { ApiToken, ApiTokenInput } from '../src/api/tokens';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { SIGNED_IN_USER, createdTokenMetadata, fakeSecret, tokenFixture } from '../src/features/tokens/testFixtures';
import { ENDPOINT_ID, instanceWith, mockApi } from './support/mockApi';

const TOKENS_PATH = '/settings/tokens';
const WIDTHS = [
  { width: 360, height: 740 },
  { width: 1024, height: 768 },
  { width: 1440, height: 900 },
] as const;

interface TokenFailure {
  status: number;
  body: unknown;
  headers?: Record<string, string>;
}

interface TokenApiState {
  tokens: ApiToken[];
  posted: ApiTokenInput[];
  revoked: string[];
  failure: TokenFailure | null;
}

const ACTIVE = tokenFixture();
const EXPIRED = tokenFixture({
  id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a32',
  name: 'old-script',
  scopes: ['endpoints:write'],
  expires_at: '2026-01-01T00:00:00.000Z',
});
const REVOKED = tokenFixture({
  id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a33',
  name: 'laptop',
  endpoint_ids: [ENDPOINT_ID],
  revoked_at: '2026-11-20T11:00:00.000Z',
});

function fulfillJson(route: Route, status: number, body: unknown, headers: Record<string, string> = {}): Promise<void> {
  return route.fulfill({ status, contentType: 'application/json', headers, body: JSON.stringify(body) });
}

async function handleTokens(route: Route, state: TokenApiState): Promise<void> {
  const request = route.request();
  const path = new URL(request.url()).pathname;
  if (request.method() === 'GET') {
    await fulfillJson(route, 200, { items: state.tokens });
    return;
  }
  if (request.method() === 'POST') {
    if (state.failure !== null) {
      await fulfillJson(route, state.failure.status, state.failure.body, state.failure.headers);
      return;
    }
    const input = request.postDataJSON() as ApiTokenInput;
    state.posted.push(input);
    const metadata = createdTokenMetadata(input);
    state.tokens = [metadata, ...state.tokens];
    await fulfillJson(route, 201, { ...metadata, token: fakeSecret() });
    return;
  }
  const id = path.split('/').pop() ?? '';
  state.revoked.push(id);
  state.tokens = state.tokens.map((token) =>
    token.id === id ? { ...token, revoked_at: '2026-11-21T10:30:00.000Z' } : token,
  );
  await route.fulfill({ status: 204 });
}

async function openTokens(page: Page, tokens: ApiToken[], failure: TokenFailure | null = null) {
  const state: TokenApiState = { tokens, posted: [], revoked: [], failure };
  await mockApi(page, {
    'GET /instance': instanceWith({ auth_mode: 'mixed', user: SIGNED_IN_USER }),
    'GET /endpoints': { body: { items: [{ ...endpointFixture, id: ENDPOINT_ID }], next_cursor: null } },
  });
  await page.route('**/api/v1/tokens**', (route) => handleTokens(route, state));
  await page.goto(TOKENS_PATH);
  return state;
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

async function storedText(page: Page): Promise<string> {
  return page.evaluate(() =>
    JSON.stringify({ local: { ...window.localStorage }, session: { ...window.sessionStorage } }),
  );
}

test('creates a token, shows the secret once, and never leaves it in storage, the URL or the page', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  const state = await openTokens(page, []);
  await expect(page.getByText('No API tokens yet')).toBeVisible();

  await page.getByRole('button', { name: 'Create token' }).click();
  const dialog = page.getByRole('dialog', { name: 'Create token' });
  await dialog.getByLabel('Name', { exact: true }).fill('ci-runner');
  await dialog.getByRole('checkbox', { name: 'Rules write' }).check();
  await dialog.getByLabel('Expires', { exact: true }).selectOption('30');
  await dialog.getByRole('button', { name: 'Create', exact: true }).click();

  const field = page.getByLabel('API token secret');
  await expect(page.getByRole('dialog', { name: 'Token created' })).toBeVisible();
  await expect(field).toBeFocused();
  await expect(field).not.toHaveValue(/FAKE/);
  expect(state.posted).toEqual([{ name: 'ci-runner', scopes: ['read', 'rules:write'], expires_in_days: 30 }]);

  await page.getByRole('button', { name: 'Show' }).click();
  await expect(field).toHaveValue(fakeSecret());
  await page.getByRole('button', { name: 'Copy', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Copied' })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(fakeSecret());
  expect(await storedText(page)).not.toContain(fakeSecret());
  expect(page.url()).not.toContain(fakeSecret());

  await page.getByRole('button', { name: 'Done' }).click();
  await expect(page.getByRole('dialog')).toBeHidden();
  await expect(page.getByRole('table', { name: 'API tokens' }).getByText('ci-runner')).toBeVisible();
  expect(await page.content()).not.toContain(fakeSecret());
  expect(await storedText(page)).not.toContain(fakeSecret());
  expect(page.url()).not.toContain(fakeSecret());
  await expect(page.getByRole('button', { name: 'Create token' })).toBeFocused();
});

test('closing the secret before copying asks first and Go back keeps it', async ({ page }) => {
  await openTokens(page, []);
  await page.getByRole('button', { name: 'Create token' }).click();
  await page.getByLabel('Name', { exact: true }).fill('ci');
  await page.getByRole('button', { name: 'Create', exact: true }).click();
  await expect(page.getByLabel('API token secret')).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(page.getByText('Close without copying?')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Go back' })).toBeFocused();
  await page.getByRole('button', { name: 'Go back' }).click();
  await expect(page.getByLabel('API token secret')).toBeVisible();

  await page.getByRole('button', { name: 'Done' }).click();
  await page.getByRole('button', { name: 'Close anyway' }).click();
  await expect(page.getByRole('dialog')).toBeHidden();
});

test('restricts a token to chosen endpoints', async ({ page }) => {
  const state = await openTokens(page, []);
  await page.getByRole('button', { name: 'Create token' }).click();
  await page.getByLabel('Name', { exact: true }).fill('only-one');
  await page.getByRole('radio', { name: 'Only the endpoints I choose' }).check();
  await page.getByRole('button', { name: 'Create', exact: true }).click();
  await expect(page.getByText('Choose at least one endpoint.')).toBeVisible();

  await page.getByRole('checkbox', { name: /stripe test/ }).check();
  await expect(page.getByText('1 of 50 selected')).toBeVisible();
  await page.getByRole('button', { name: 'Create', exact: true }).click();

  await expect(page.getByLabel('API token secret')).toBeVisible();
  expect(state.posted[0]?.endpoint_ids).toEqual([ENDPOINT_ID]);
});

test('revokes an active token after confirmation and keeps focus on the page heading', async ({ page }) => {
  const state = await openTokens(page, [ACTIVE, EXPIRED, REVOKED]);
  const table = page.getByRole('table', { name: 'API tokens' });
  await expect(table.getByRole('row')).toHaveCount(4);
  await expect(page.getByRole('button', { name: /^Revoke / })).toHaveCount(1);

  await page.getByRole('button', { name: 'Revoke ci-runner' }).click();
  const dialog = page.getByRole('dialog', { name: 'Revoke ci-runner?' });
  await dialog.getByRole('button', { name: 'Revoke', exact: true }).click();

  await expect(page.getByText('Token ci-runner was revoked.')).toBeVisible();
  await expect.poll(() => state.revoked).toEqual([ACTIVE.id]);
  await expect(page.getByRole('button', { name: /^Revoke / })).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 1, name: 'API tokens' })).toBeFocused();
});

test('explains the token limit and the hourly creation quota', async ({ page }) => {
  const state = await openTokens(page, [], {
    status: 422,
    body: { error: { code: 'limit_reached', message: 'limit', limit: { name: 'tokens', max: 20 } } },
  });
  await page.getByRole('button', { name: 'Create token' }).click();
  await page.getByLabel('Name', { exact: true }).fill('ci');
  await page.getByRole('button', { name: 'Create', exact: true }).click();
  await expect(page.getByText('You already have 20 active tokens. Revoke one before creating another.')).toBeVisible();

  state.failure = {
    status: 429,
    headers: { 'Retry-After': '900' },
    body: {
      error: {
        code: 'quota_exceeded',
        message: 'slow',
        limit: {
          name: 'token_creation',
          window: 'hour',
          max: 10,
          resets_at: new Date(Date.now() + 15 * 60_000).toISOString(),
        },
      },
    },
  };
  await page.getByRole('button', { name: 'Create', exact: true }).click();
  await expect(page.getByText(/You can create 10 tokens per hour\. Try again in 1[45] minutes\./)).toBeVisible();
});

test('shows the not-available page when the instance has no sign-in', async ({ page }) => {
  await mockApi(page, { 'GET /instance': instanceWith() });
  await page.goto(TOKENS_PATH);

  await expect(page.getByRole('heading', { level: 1, name: 'API tokens are not available here' })).toBeVisible();
});

test('the header link opens the page for a signed-in user', async ({ page }) => {
  await openTokens(page, [ACTIVE]);
  await page.goto('/settings');

  await page.getByRole('link', { name: 'API tokens' }).click();

  await expect(page).toHaveURL(TOKENS_PATH);
  await expect(page.getByRole('table', { name: 'API tokens' })).toBeVisible();
});

for (const { width, height } of WIDTHS) {
  test(`the page and the create dialog fit at ${width} px without sideways scrolling`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openTokens(page, [ACTIVE, EXPIRED, REVOKED]);
    await expect(page.getByRole('heading', { level: 1, name: 'API tokens' })).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);
    if (width === 360) {
      await expect(page.getByRole('list', { name: 'API tokens' })).toBeVisible();
    } else {
      await expect(page.getByRole('table', { name: 'API tokens' })).toBeVisible();
    }

    await page.getByRole('button', { name: 'Create token' }).click();
    const dialog = page.getByRole('dialog', { name: 'Create token' });
    await expect(dialog).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);
    await expect.poll(async () => (await dialog.boundingBox())?.width ?? 0).toBeLessThanOrEqual(width);
  });
}

for (const theme of ['Light', 'Dark'] as const) {
  test(`the tokens page has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openTokens(page, [ACTIVE, EXPIRED, REVOKED]);
    await chooseTheme(page, theme);
    await expect(page.getByRole('table', { name: 'API tokens' })).toBeVisible();

    const result = await new AxeBuilder({ page }).analyze();
    expect(result.violations).toEqual([]);
  });

  test(`the create dialog and the secret reveal have no axe violations in the ${theme.toLowerCase()} theme`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await openTokens(page, []);
    await chooseTheme(page, theme);
    await page.getByRole('button', { name: 'Create token' }).click();
    await page.getByRole('radio', { name: 'Only the endpoints I choose' }).check();
    await expect(page.getByRole('checkbox', { name: /stripe test/ })).toBeVisible();
    await page.getByRole('button', { name: 'Create', exact: true }).click();
    await expect(page.getByText('Enter a name.')).toBeVisible();

    const form = await new AxeBuilder({ page }).analyze();
    expect(form.violations).toEqual([]);

    await page.getByLabel('Name', { exact: true }).fill('ci');
    await page.getByRole('radio', { name: 'All my endpoints' }).check();
    await page.getByRole('button', { name: 'Create', exact: true }).click();
    await expect(page.getByLabel('API token secret')).toBeVisible();

    const reveal = await new AxeBuilder({ page }).analyze();
    expect(reveal.violations).toEqual([]);
  });
}
