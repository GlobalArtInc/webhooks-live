import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const LOCAL_ID = 'k7m2q9x4w8c1d5f3h6j0n2p4';
const FOLDER_ID = 'a1b2c3d4-0000-4000-8000-000000000001';
const OWNER = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21', email: 'dev@example.test', name: 'Dev' };
const ENDPOINTS_KEY = 'webhooks-live:endpoints:v1';
const ORGANIZE_KEY = 'webhooks-live:organize:v1';
const SIGN_IN_AUTH = { provider_name: null, sign_in_available: true };
const PAYMENTS = {
  id: FOLDER_ID,
  name: 'Payments',
  position: 0,
  endpoint_ids: [ENDPOINT_ID],
  created_at: '2026-11-21T10:00:00.000Z',
};

interface BrowserSeed {
  endpoints: string;
  organize: string;
}

interface SeedKeys {
  endpoints: string;
  organize: string;
}

interface SeedInput {
  keys: SeedKeys;
  seed: BrowserSeed;
}

const ACCOUNT_ENDPOINT = {
  ...endpointFixture,
  id: ENDPOINT_ID,
  title: 'stripe-dev',
  tags: ['billing'],
  owner: OWNER,
  my_role: 'owner',
};

const LOCAL_ENDPOINT = { ...endpointFixture, id: LOCAL_ID, title: null, tags: [], owner: null, my_role: null };

function browserSeed(): BrowserSeed {
  const now = Date.now();
  return {
    endpoints: JSON.stringify({
      version: 1,
      endpoints: [{ id: LOCAL_ID, label: 'github-app', added: now, lastOpened: now, lastSeenCaptureId: null }],
    }),
    organize: JSON.stringify({
      version: 1,
      folders: [{ id: 'local-receipts', name: 'Receipts' }],
      placements: [{ id: LOCAL_ID, folderId: 'local-receipts', tags: ['ci'] }],
    }),
  };
}

async function seedBrowser(page: Page, seed: BrowserSeed): Promise<void> {
  await page.addInitScript(
    (input: SeedInput) => {
      if (window.sessionStorage.getItem('seeded') === null) {
        window.localStorage.setItem(input.keys.endpoints, input.seed.endpoints);
        window.localStorage.setItem(input.keys.organize, input.seed.organize);
        window.sessionStorage.setItem('seeded', '1');
      }
    },
    { keys: { endpoints: ENDPOINTS_KEY, organize: ORGANIZE_KEY }, seed },
  );
}

function signedInReplies(extra: ReplyMap = {}): ReplyMap {
  return {
    'GET /instance': instanceWith({ auth_mode: 'mixed', user: OWNER, auth: SIGN_IN_AUTH }),
    'GET /endpoints': { body: { items: [ACCOUNT_ENDPOINT], next_cursor: null } },
    'GET /folders': { body: { items: [PAYMENTS] } },
    ...extra,
  };
}

async function expectNoSeriousIssues(page: Page): Promise<void> {
  const results = await new AxeBuilder({ page }).analyze();
  const serious = results.violations.filter(
    (violation) => violation.impact === 'serious' || violation.impact === 'critical',
  );
  expect(serious).toEqual([]);
}

test('My endpoints lists an account endpoint and a browser endpoint once each, with their sources', async ({
  page,
}) => {
  await mockApi(page, signedInReplies());
  await seedBrowser(page, browserSeed());

  await page.goto('/endpoints');

  const main = page.getByRole('main');
  await expect(main.getByRole('heading', { level: 1, name: 'My endpoints' })).toBeVisible();
  await expect(main.getByRole('link', { name: 'stripe-dev' })).toHaveCount(1);
  await expect(main.getByRole('link', { name: 'github-app' })).toHaveCount(1);
  await expect(
    main.getByRole('listitem').filter({ has: page.getByRole('link', { name: 'stripe-dev' }) }),
  ).toContainText('Account');
  await expect(
    main.getByRole('listitem').filter({ has: page.getByRole('link', { name: 'github-app' }) }),
  ).toContainText('This browser');
});

test('a label chip narrows the page to the endpoints with that label', async ({ page }) => {
  await mockApi(page, signedInReplies());
  await seedBrowser(page, browserSeed());
  await page.goto('/endpoints');

  await page.getByRole('main').getByRole('button', { name: 'billing: 1 endpoint' }).click();

  await expect(page).toHaveURL(/label=billing/);
  await expect(page.getByRole('main').getByRole('button', { name: 'billing: 1 endpoint' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
});

test('a folder created from the keyboard in the rail appears as a group', async ({ page }) => {
  await mockApi(page, {
    'GET /instance': instanceWith({ auth_mode: 'mixed', auth: SIGN_IN_AUTH }),
  });
  await page.goto('/');

  const newFolder = page.getByRole('button', { name: 'New folder', exact: true });
  await newFolder.focus();
  await page.keyboard.press('Enter');
  await page.keyboard.type('Receipts');
  await page.keyboard.press('Enter');

  await expect(page.getByRole('button', { name: 'Rename folder Receipts' })).toBeVisible();
  await expect(newFolder).toBeFocused();
});

test('moving a browser endpoint to the account keeps its label and leaves the browser list', async ({ page }) => {
  await mockApi(
    page,
    signedInReplies({
      'GET /endpoints': { body: { items: [ACCOUNT_ENDPOINT], next_cursor: null } },
      'GET /folders': { body: { items: [] } },
      [`POST /endpoints/${LOCAL_ID}/claim`]: { body: { ...LOCAL_ENDPOINT, owner: OWNER, my_role: 'owner' } },
      [`PATCH /endpoints/${LOCAL_ID}`]: {
        body: { ...LOCAL_ENDPOINT, title: 'github-app', owner: OWNER, my_role: 'owner' },
      },
      'POST /folders': {
        status: 201,
        body: {
          id: FOLDER_ID,
          name: 'Receipts',
          position: 0,
          endpoint_ids: [],
          created_at: '2026-11-21T10:00:00.000Z',
        },
      },
      [`PUT /folders/${FOLDER_ID}/endpoints/${LOCAL_ID}`]: { status: 200, body: {} },
    }),
  );
  await seedBrowser(page, browserSeed());
  await page.goto('/endpoints');

  const main = page.getByRole('main');
  await main.getByRole('button', { name: 'Move to account: github-app' }).click();

  await expect(main.getByText('Moved to your account.')).toBeVisible();
  await expect(main.getByRole('link', { name: 'github-app' })).toHaveCount(0);
});

test('the rail and the My endpoints page fit a 360 px viewport without horizontal scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await mockApi(page, signedInReplies());
  await seedBrowser(page, browserSeed());

  await page.goto('/endpoints');
  await expect(page.getByRole('main').getByRole('heading', { level: 1, name: 'My endpoints' })).toBeVisible();

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});

for (const colorScheme of ['light', 'dark'] as const) {
  test(`the organize screens have no serious accessibility issues in ${colorScheme} theme`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await mockApi(page, signedInReplies());
    await seedBrowser(page, browserSeed());

    await page.goto('/endpoints');
    await expect(page.getByRole('main').getByRole('heading', { level: 1, name: 'My endpoints' })).toBeVisible();
    await expectNoSeriousIssues(page);

    await page.goto('/');
    await expect(page.getByRole('button', { name: 'New folder', exact: true })).toBeVisible();
    await expectNoSeriousIssues(page);
  });
}
