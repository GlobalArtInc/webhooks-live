import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { instanceFixture, PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { apiError, ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const BASE_PATH = `/e/${ENDPOINT_ID}`;
const WORKSPACE_NAV = 'Endpoint configuration';
const WIDTHS = [
  { width: 360, height: 740 },
  { width: 1024, height: 768 },
  { width: 1440, height: 900 },
] as const;
const READY_SECTIONS = [
  { id: 'general', heading: 'General' },
  { id: 'response', heading: 'Response' },
  { id: 'forwarding', heading: 'Forwarding' },
  { id: 'rules', heading: 'Rules' },
  { id: 'access', heading: 'Access' },
  { id: 'schedules', heading: 'Schedules' },
] as const;
const PLANNED_SECTIONS = ['notifications', 'cli'] as const;
const TARGET_REFUSED = 'target_url resolves only to refused addresses';

function endpointReplies(extra: ReplyMap = {}): ReplyMap {
  return {
    'GET /instance': instanceWith({ features: { ...instanceFixture.features, schedules: true } }),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}` },
    },
    [`GET /endpoints/${ENDPOINT_ID}/rules`]: { body: { items: [], version: 1 } },
    [`GET /endpoints/${ENDPOINT_ID}/schedules`]: { body: { items: [] } },
    ...extra,
  };
}

async function openWorkspace(page: Page, path: string, extra: ReplyMap = {}): Promise<void> {
  await mockApi(page, endpointReplies(extra));
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await installFakeStream(page);
  await page.goto(path);
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

for (const { width, height } of WIDTHS) {
  test(`every workspace section opens at ${width} px without sideways scrolling`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openWorkspace(page, `${BASE_PATH}/settings/general`);

    for (const section of READY_SECTIONS) {
      await page.goto(`${BASE_PATH}/settings/${section.id}`);
      await expect(page.getByRole('heading', { level: 2, name: section.heading })).toBeVisible();
      expect(await horizontalOverflow(page)).toBe(0);
    }
    for (const section of PLANNED_SECTIONS) {
      await page.goto(`${BASE_PATH}/settings/${section}`);
      await expect(page.getByText('This section arrives in a later release.')).toBeVisible();
    }
  });
}

test('the bare settings path opens General on wide screens and the section list on narrow ones', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openWorkspace(page, `${BASE_PATH}/settings`);
  await expect(page).toHaveURL(`${BASE_PATH}/settings/general`);

  await page.setViewportSize({ width: 360, height: 740 });
  await page.goto(`${BASE_PATH}/settings`);
  await expect(page.getByRole('navigation', { name: WORKSPACE_NAV })).toBeVisible();
  await expect(page).toHaveURL(`${BASE_PATH}/settings`);
});

test('the old forwarding path redirects to the forwarding section', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openWorkspace(page, `${BASE_PATH}/forwarding`);
  await expect(page).toHaveURL(`${BASE_PATH}/settings/forwarding`);
  await expect(page.getByRole('heading', { level: 2, name: 'Forwarding' })).toBeVisible();
});

test('leaving a section with unsaved changes asks first, and Discard then leaves', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openWorkspace(page, `${BASE_PATH}/settings/general`);
  await page.getByLabel('Title').fill('renamed');

  await page.getByRole('navigation', { name: WORKSPACE_NAV }).getByRole('link', { name: 'Response' }).click();
  const dialog = page.getByRole('dialog', { name: 'Discard unsaved changes?' });
  await expect(dialog).toBeVisible();
  await expect(page).toHaveURL(`${BASE_PATH}/settings/general`);

  await dialog.getByRole('button', { name: 'Discard' }).click();
  await expect(page).toHaveURL(`${BASE_PATH}/settings/response`);
});

test('a refused forward target lands on its field', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openWorkspace(page, `${BASE_PATH}/settings/forwarding`, {
    [`POST /endpoints/${ENDPOINT_ID}/forward-rules`]: apiError(422, 'validation_failed', TARGET_REFUSED, [
      { field: 'target_url', code: 'target_refused', message: '10.0.0.5 is a private address' },
    ]),
  });

  await page.getByRole('button', { name: 'Add rule' }).click();
  await page.getByLabel('Target URL').fill('http://10.0.0.5/hook');
  await page.getByRole('button', { name: 'Save rule' }).click();

  await expect(page.getByLabel('Target URL')).toHaveAttribute('aria-invalid', 'true');
  await expect(page.getByText('This address is refused. 10.0.0.5 is a private address')).toBeVisible();
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the response section has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openWorkspace(page, `${BASE_PATH}/settings/response`);
    await chooseTheme(page, theme);
    await expect(page.getByRole('heading', { level: 2, name: 'Response' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the general section has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openWorkspace(page, `${BASE_PATH}/settings/general`);
    await chooseTheme(page, theme);
    await expect(page.getByRole('heading', { level: 2, name: 'General' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
