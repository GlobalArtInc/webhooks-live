import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints, type MyEndpoint } from '../src/features/endpoints/storage';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { ENDPOINT_ID, instanceWith, mockApi, type Reply } from './support/mockApi';

const WIDTHS = [1440, 1024, 768, 360] as const;
const WIDE = 1440;
const NARROW = 360;
const MIN_CTA_HEIGHT = 56;
const NOW = Date.now();
const STORED: MyEndpoint = {
  id: ENDPOINT_ID,
  label: 'stripe-dev',
  added: NOW - 86400000,
  lastOpened: NOW - 600000,
  lastSeenCaptureId: null,
};
const CREATED: Reply = {
  status: 201,
  body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}`, title: null },
};

async function openHome(page: Page, width: number, stored: readonly MyEndpoint[] = []): Promise<void> {
  await page.addInitScript(
    ([key, value]) => {
      if (window.localStorage.getItem(key as string) === null) {
        window.localStorage.setItem(key as string, value as string);
      }
    },
    [ENDPOINTS_STORAGE_KEY, serializeEndpoints(stored)],
  );
  await page.setViewportSize({ width, height: 900 });
  await mockApi(page, {
    'GET /instance': instanceWith(),
    'POST /endpoints': CREATED,
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: CREATED.body },
  });
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1, name: 'Watch HTTP requests arrive live.' })).toBeVisible();
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

for (const width of [WIDE, NARROW]) {
  test(`at ${width} px the headline, the create button and the hero lines follow the design`, async ({ page }) => {
    await openHome(page, width);

    const title = page.getByRole('heading', { level: 1 });
    const fontSize = await title.evaluate((node) => parseFloat(getComputedStyle(node).fontSize));
    expect(fontSize).toBe(width === WIDE ? 52 : 36);
    const lines = await title.evaluate((node) => {
      const box = node.getBoundingClientRect();
      return Math.round(box.height / parseFloat(getComputedStyle(node).lineHeight));
    });
    expect(lines).toBeLessThanOrEqual(width === WIDE ? 2 : 3);

    const button = page.getByRole('button', { name: 'Create endpoint' });
    const box = await button.boundingBox();
    expect(box?.height ?? 0).toBeGreaterThanOrEqual(MIN_CTA_HEIGHT);
    if (width === NARROW) {
      const main = await page.locator('main').boundingBox();
      expect(box?.width ?? 0).toBeGreaterThanOrEqual((main?.width ?? 0) - 48);
    }
  });
}

for (const width of WIDTHS) {
  test(`at ${width} px the home page has no horizontal overflow, with and without endpoints`, async ({ page }) => {
    await openHome(page, width, [STORED]);

    expect(await horizontalOverflow(page)).toBe(0);
    await expect(page.getByRole('main').getByRole('heading', { level: 2, name: 'My endpoints' })).toBeVisible();
  });

  for (const theme of ['Light', 'Dark'] as const) {
    test(`at ${width} px the home page has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
      await openHome(page, width, [STORED]);
      await page.evaluate((value) => {
        document.documentElement.setAttribute('data-theme', value);
      }, theme.toLowerCase());

      const result = await new AxeBuilder({ page }).analyze();
      expect(result.violations).toEqual([]);
    });
  }
}

test('the curl example uses a placeholder without endpoints and copies the exact command', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await openHome(page, WIDE);
  const expected = `curl -X POST ${PUBLIC_URL}/YOUR_ENDPOINT_ID/hello -d '{"hello":"world"}'`;

  await expect(page.getByText('Then send a request')).toBeVisible();
  await expect(page.getByText(expected, { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Copy the curl command' }).click();

  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(expected);
  await expect(page.getByRole('status').filter({ hasText: 'Copied' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Copy the curl command' })).toContainText('Copied');
});

test('with a stored endpoint the curl example and its card use the real capture URL and Open works', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await openHome(page, WIDE, [STORED]);
  const url = `${PUBLIC_URL}/${ENDPOINT_ID}`;
  const expected = `curl -X POST ${url}/hello -d '{"hello":"world"}'`;

  await expect(page.getByText('Send a request to your latest endpoint')).toBeVisible();
  await expect(page.getByText(expected, { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Copy the curl command' }).click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(expected);

  const card = page.getByRole('main').getByRole('listitem').filter({ hasText: 'stripe-dev' });
  await expect(card.locator('[title^="http"]')).toHaveAttribute('title', url);
  await card.getByRole('button', { name: 'Copy URL of stripe-dev' }).click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(url);

  await card.getByRole('link', { name: 'Open stripe-dev' }).click();
  await expect(page).toHaveURL(`/e/${ENDPOINT_ID}`);
});

test('Enter on the focused create button still creates and opens an endpoint', async ({ page }) => {
  await openHome(page, WIDE);

  await page.getByRole('button', { name: 'Create endpoint' }).focus();
  await page.keyboard.press('Enter');

  await expect(page).toHaveURL(`/e/${ENDPOINT_ID}`);
});

test('the empty state says where endpoints are saved', async ({ page }) => {
  await openHome(page, WIDE);

  await expect(
    page.getByText('No endpoints yet. Endpoints you create appear here, saved in this browser only.'),
  ).toBeVisible();
});

for (const width of [WIDE, NARROW]) {
  test(`at ${width} px every button and link in an endpoint card is at least 44 px high`, async ({ page }) => {
    await openHome(page, width, [STORED]);

    const card = page.getByRole('main').getByRole('listitem').filter({ hasText: 'stripe-dev' });
    const heights = await card
      .locator('a, button')
      .evaluateAll((elements) => elements.map((element) => element.getBoundingClientRect().height));
    expect(heights).toHaveLength(4);
    for (const height of heights) {
      expect(height).toBeGreaterThanOrEqual(44);
    }
  });
}
