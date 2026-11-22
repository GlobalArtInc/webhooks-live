import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints, type MyEndpoint } from '../src/features/endpoints/storage';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi } from './support/mockApi';

const MIN_TARGET_PX = 44;
const MIN_FONT_PX = 14;
const SECOND_ID = 'k7m2q9x4w8c1d5f3h6j0n2p1';
const THIRD_ID = 'k7m2q9x4w8c1d5f3h6j0n2p2';
const RAIL_NAMES = [
  'Expand sidebar',
  'New endpoint',
  'My endpoints',
  'Composer',
  'Folders',
  'Labels',
  'stripe-dev',
  'github hooks',
  'k7m2q9...2p2',
];

const ENTRIES: MyEndpoint[] = [
  { id: ENDPOINT_ID, label: 'stripe-dev', added: 1, lastOpened: 10, lastSeenCaptureId: null },
  { id: SECOND_ID, label: 'github hooks', added: 1, lastOpened: 9, lastSeenCaptureId: null },
  { id: THIRD_ID, label: null, added: 1, lastOpened: 8, lastSeenCaptureId: null },
];

async function openRail(
  page: Page,
  theme: 'light' | 'dark' = 'light',
  path = `/e/${ENDPOINT_ID}/settings/response`,
  seed = true,
): Promise<Locator> {
  await page.addInitScript(
    ([endpointsKey, endpoints, theme, seeded]) => {
      if (seeded === 'yes' && window.localStorage.getItem(endpointsKey as string) === null) {
        window.localStorage.setItem(endpointsKey as string, endpoints as string);
        window.localStorage.setItem('webhooks-live:prefs:v1', JSON.stringify({ theme }));
      }
    },
    [ENDPOINTS_STORAGE_KEY, serializeEndpoints(ENTRIES), theme, seed ? 'yes' : 'no'],
  );
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeStream(page);
  await mockApi(page, {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}` },
    },
    'GET /folders': { body: { items: [] } },
    [`GET /endpoints/${ENDPOINT_ID}/aliases`]: { body: { items: [] } },
  });
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await page.goto(path);
  const rail = page.getByRole('navigation', { name: 'My endpoints' });
  await expect(rail.getByRole('link', { name: 'stripe-dev' })).toBeVisible();
  return rail;
}

async function focusedName(page: Page): Promise<string> {
  return page.evaluate(() => {
    const element = document.activeElement;
    const names = Array.from(element?.querySelectorAll('span') ?? [])
      .filter((span) => span.getAttribute('aria-hidden') !== 'true')
      .map((span) => span.textContent ?? '');
    return names.join('').trim();
  });
}

test('every control of the collapsed rail is at least 44 by 44 px and no text in it is under 14 px', async ({
  page,
}) => {
  const rail = await openRail(page);

  const boxes = await rail.locator('a:visible, button:visible').evaluateAll((elements) =>
    elements.map((element) => {
      const box = element.getBoundingClientRect();
      const name = Array.from(element.querySelectorAll('span'))
        .filter((span) => span.getAttribute('aria-hidden') !== 'true')
        .map((span) => span.textContent ?? '')
        .join('')
        .trim();
      return { name, width: box.width, height: box.height };
    }),
  );
  expect(boxes.map((box) => box.name)).toEqual(RAIL_NAMES);
  for (const box of boxes) {
    expect(box.width, box.name).toBeGreaterThanOrEqual(MIN_TARGET_PX);
    expect(box.height, box.name).toBeGreaterThanOrEqual(MIN_TARGET_PX);
  }

  const smallest = await rail.evaluate((element) => {
    const sizes: number[] = [];
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      const parent = node.parentElement;
      const box = parent?.getBoundingClientRect();
      if ((node.textContent ?? '').trim() !== '' && parent !== null && box !== undefined && box.width > 1) {
        sizes.push(parseFloat(getComputedStyle(parent).fontSize));
      }
    }
    return Math.min(...sizes);
  });
  expect(smallest).toBeGreaterThanOrEqual(MIN_FONT_PX);
  expect(await rail.locator('svg').count()).toBeGreaterThanOrEqual(6);
  expect(await rail.locator('svg:not([aria-hidden="true"])').count()).toBe(0);
});

test('Tab walks the rail in order, focus shows the tooltip, Escape hides it and Enter works on each item', async ({
  page,
}) => {
  const rail = await openRail(page);
  await expect(rail.getByRole('button', { name: 'Expand sidebar' })).toBeVisible();
  const expand = rail.getByRole('button', { name: 'Expand sidebar' });
  await expand.focus();
  await page.keyboard.press('Tab');
  await page.keyboard.press('Shift+Tab');
  await expect(expand).toBeFocused();
  await expect(page.getByText('Expand sidebar', { exact: true }).and(page.locator('[class*="bubble"]'))).toBeVisible();

  const visited: string[] = [await focusedName(page)];
  for (let index = 1; index < RAIL_NAMES.length; index += 1) {
    await page.keyboard.press('Tab');
    visited.push(await focusedName(page));
  }
  expect(visited).toEqual(RAIL_NAMES);

  await page.getByRole('link', { name: 'stripe-dev' }).focus();
  await page.keyboard.press('Tab');
  await page.keyboard.press('Shift+Tab');
  const tip = page.locator('[class*="bubble"]');
  await expect(tip).toHaveText('stripe-dev');
  await page.keyboard.press('Escape');
  await expect(tip).toBeHidden();

  await rail.getByRole('button', { name: 'Folders' }).focus();
  await page.keyboard.press('Enter');
  await expect(rail.getByRole('button', { name: 'Collapse sidebar' })).toBeVisible();
});

test('Enter on the Composer item opens the composer and Enter on an endpoint opens it', async ({ page }) => {
  const rail = await openRail(page);

  await rail.getByRole('link', { name: 'Composer' }).focus();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/send$/);

  await page.goto(`/e/${ENDPOINT_ID}/settings/response`);
  await expect(
    page.getByRole('navigation', { name: 'My endpoints' }).getByRole('link', { name: 'stripe-dev' }),
  ).toBeVisible();
  await page.getByRole('link', { name: 'github hooks' }).focus();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(new RegExp(`/e/${SECOND_ID}$`));
});

test('the open endpoint and the open section are marked as the current page', async ({ page }) => {
  const rail = await openRail(page);

  await expect(rail.getByRole('link', { name: 'stripe-dev' })).toHaveAttribute('aria-current', 'page');
  await expect(rail.getByRole('link', { name: 'github hooks' })).not.toHaveAttribute('aria-current', /.*/);
});

for (const theme of ['light', 'dark'] as const) {
  test(`the collapsed rail has no axe violations in the ${theme} theme`, async ({ page }) => {
    await openRail(page, theme);

    const result = await new AxeBuilder({ page }).analyze();
    expect(result.violations).toEqual([]);
  });
}

test('the rail keeps the manual choice across reloads, collapses on Response and returns to the stored choice', async ({
  page,
}) => {
  const rail = await openRail(page, 'light', `/e/${ENDPOINT_ID}`);
  const collapse = rail.getByRole('button', { name: 'Collapse sidebar' });
  await expect(collapse).toHaveAttribute('aria-expanded', 'true');

  await collapse.click();
  const expand = rail.getByRole('button', { name: 'Expand sidebar' });
  await expect(expand).toHaveAttribute('aria-expanded', 'false');

  await page.reload();
  await expect(rail.getByRole('button', { name: 'Expand sidebar' })).toBeVisible();

  await page.goto(`/e/${ENDPOINT_ID}/settings/response`);
  await expect(rail.getByRole('button', { name: 'Expand sidebar' })).toBeVisible();

  await page.getByRole('link', { name: 'Back to captures' }).click();
  await expect(page).toHaveURL(new RegExp(`/e/${ENDPOINT_ID}$`));
  await expect(rail.getByRole('button', { name: 'Expand sidebar' })).toBeVisible();

  await rail.getByRole('button', { name: 'Expand sidebar' }).click();
  await expect(rail.getByRole('button', { name: 'Collapse sidebar' })).toBeVisible();
  await page.reload();
  await expect(rail.getByRole('button', { name: 'Collapse sidebar' })).toBeVisible();
});

test('a manual expand on the Response route is not stored', async ({ page }) => {
  const rail = await openRail(page);

  await rail.getByRole('button', { name: 'Expand sidebar' }).click();
  await expect(rail.getByRole('button', { name: 'Collapse sidebar' })).toBeVisible();

  await page.getByRole('link', { name: 'Back to captures' }).click();
  await expect(rail.getByRole('button', { name: 'Collapse sidebar' })).toBeVisible();
  await page.goto(`/e/${ENDPOINT_ID}/settings/response`);
  await expect(rail.getByRole('button', { name: 'Expand sidebar' })).toBeVisible();
});
