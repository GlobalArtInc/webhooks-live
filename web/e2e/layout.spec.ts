import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import type { Capture } from '../src/api/types';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { summaryOf } from '../src/test/msw/store';
import { mockCaptureApi, type CaptureApiState } from './support/captureApi';
import { installFakeStream, openLatestStream, pushStreamEvent, streamCount } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const WIDE = { width: 1440, height: 900 };
const NARROW = { width: 360, height: 740 };
const BIG_BODY_LENGTH = 1_000_000;
const HEADER_VALUE_LENGTH = 40_000;
const QUERY_LENGTH = 5000;
const MIN_PATH_CHARS = 12;
const NARROW_ROW_HEIGHT = 56;
const MIN_VISIBLE_ROWS = 4;
const CLOCK_AHEAD_MS = 8000;
const LONG_PATH = '/webhooks/stripe/payment-intents/succeeded/evt_1Q9';

interface Size {
  width: number;
  height: number;
}

function endpointReplies(captureCount: number): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: {
        ...endpointFixture,
        id: ENDPOINT_ID,
        url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
        capture_count: captureCount,
        last_seq: captureCount,
      },
    },
  };
}

function oneLine(length: number): string {
  return 'zxV2CW8Gvrcl+YZMGAMbB1Ll1DJNSMcl6v9mp2Z358ymCHiOYIEhvrgf'.repeat(Math.ceil(length / 56)).slice(0, length);
}

function bigCapture(): Capture {
  const [base] = captureSeries(1);
  return {
    ...(base as Capture),
    content_type: 'text/plain',
    path: '/big',
    body_size: BIG_BODY_LENGTH,
    headers: [
      { name: 'Host', value: 'webhooks.live' },
      { name: 'x-long', value: oneLine(HEADER_VALUE_LENGTH) },
    ],
    query_string: `a=${oneLine(QUERY_LENGTH)}`,
    body: { encoding: 'utf8', data: oneLine(BIG_BODY_LENGTH), truncated: false },
  };
}

function longPathSeries(count: number): Capture[] {
  return captureSeries(count).map((capture) => ({ ...capture, path: `${LONG_PATH}/${capture.seq}` }));
}

async function openEndpoint(
  page: Page,
  size: Size,
  captures: Capture[],
  path = `/e/${ENDPOINT_ID}`,
): Promise<CaptureApiState> {
  await page.setViewportSize(size);
  await installFakeStream(page);
  await mockApi(page, endpointReplies(captures.length));
  const state = await mockCaptureApi(page, ENDPOINT_ID, captures);
  await page.goto(path);
  return state;
}

function listboxOf(page: Page): Locator {
  return page.getByRole('listbox', { name: 'Captures' });
}

async function openLive(page: Page, size: Size, captures: Capture[]): Promise<CaptureApiState> {
  const state = await openEndpoint(page, size, captures);
  await expect(listboxOf(page)).toBeVisible();
  await expect.poll(() => streamCount(page)).toBeGreaterThan(0);
  await openLatestStream(page);
  await expect(page.getByText('Live', { exact: true })).toBeVisible();
  return state;
}

async function documentOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

async function firstRowTop(page: Page): Promise<number> {
  const box = await listboxOf(page).getByRole('option').first().boundingBox();
  return box?.y ?? Number.POSITIVE_INFINITY;
}

for (const size of [WIDE, NARROW]) {
  test(`a one-line 1 MB body, a 40 KB header value and a long query keep the page inside ${size.width} px`, async ({
    page,
  }) => {
    const capture = bigCapture();
    await openEndpoint(page, size, [capture], `/e/${ENDPOINT_ID}/c/${capture.id}`);

    await expect(page.getByRole('heading', { name: 'Body', exact: true })).toBeVisible();
    await expect(page.locator('main .cm-editor').first()).toBeVisible();

    const measure = async () => {
      const overflow = await documentOverflow(page);
      const editor = await page.locator('main .cm-editor').first().boundingBox();
      const main = await page.locator('main').evaluate((node) => node.scrollWidth - node.clientWidth);
      return { overflow, main, editorRight: (editor?.x ?? 0) + (editor?.width ?? 0) };
    };

    const pretty = await measure();
    expect(pretty.overflow).toBe(0);
    expect(pretty.main).toBe(0);
    expect(pretty.editorRight).toBeLessThanOrEqual(size.width);
    const scrolls = await page
      .locator('main .cm-scroller')
      .first()
      .evaluate((node) => node.scrollWidth > node.clientWidth);
    expect(scrolls).toBe(true);

    await page.getByRole('button', { name: 'Raw', exact: true }).click();
    const raw = await measure();
    expect(raw.overflow).toBe(0);
    expect(raw.main).toBe(0);
    expect(raw.editorRight).toBeLessThanOrEqual(size.width);

    const table = page.locator('main table').last();
    await expect(table).toBeVisible();
    const tableRight = await table.evaluate((node) => node.getBoundingClientRect().right);
    expect(tableRight).toBeLessThanOrEqual(size.width);
    if (size === WIDE) {
      const settings = await page.getByRole('link', { name: 'Settings' }).boundingBox();
      expect((settings?.x ?? 0) + (settings?.width ?? 0)).toBeLessThanOrEqual(size.width);
    }
  });
}

test('at 360 the row actions sit behind a menu and the path keeps at least twelve characters', async ({ page }) => {
  await openLive(page, NARROW, longPathSeries(30));
  const row = listboxOf(page).getByRole('option').first();

  const pathWidth = await row.locator('[title^="/webhooks"]').evaluate((node) => node.clientWidth);
  const glyphWidth = await row.locator('[title^="/webhooks"]').evaluate((node) => {
    const probe = document.createElement('span');
    probe.style.font = getComputedStyle(node).font;
    probe.style.position = 'absolute';
    probe.textContent = 'x'.repeat(12);
    document.body.append(probe);
    const width = probe.getBoundingClientRect().width / 12;
    probe.remove();
    return width;
  });
  expect(pathWidth / glyphWidth).toBeGreaterThanOrEqual(MIN_PATH_CHARS);
  const overflows = await row
    .locator('[title^="/webhooks"]')
    .evaluate((node) => getComputedStyle(node).textOverflow === 'ellipsis' && node.scrollWidth > node.clientWidth);
  expect(overflows).toBe(true);

  await expect(page.getByRole('button', { name: 'Delete capture' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Mark for compare' })).toHaveCount(0);
  const menuButton = page.getByRole('button', { name: 'Capture actions' }).first();
  await menuButton.click();
  await expect(page.getByRole('menuitem', { name: 'Mark for compare' })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'Delete capture' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(menuButton).toBeFocused();
});

test('at 1440 the row actions stay inline buttons', async ({ page }) => {
  await openLive(page, WIDE, longPathSeries(10));

  await expect(page.getByRole('button', { name: 'Delete capture' }).first()).toBeVisible();
  await expect(page.getByRole('button', { name: 'Capture actions' })).toHaveCount(0);
});

test('at 360 the chrome is compact, the URL is truncated in the middle and the filters open as a sheet', async ({
  page,
}) => {
  await openLive(page, NARROW, longPathSeries(30));

  const strip = await page.getByRole('banner').boundingBox();
  expect(strip?.height ?? Number.POSITIVE_INFINITY).toBeLessThanOrEqual(64);

  const top = await firstRowTop(page);
  expect(top + MIN_VISIBLE_ROWS * NARROW_ROW_HEIGHT).toBeLessThanOrEqual(NARROW.height);

  const url = page.locator(`code [title$="${ENDPOINT_ID}"]`);
  const truncated = await url
    .locator('span')
    .nth(1)
    .evaluate((node) => node.scrollWidth > node.clientWidth);
  expect(truncated).toBe(true);
  await expect(url).toContainText(ENDPOINT_ID.slice(-6));
  await expect(url).toContainText('webhooks.example.test/');
  expect(await documentOverflow(page)).toBe(0);

  const toggle = page.getByRole('button', { name: 'Filters (0)' });
  await expect(toggle).toBeVisible();
  const before = await firstRowTop(page);
  await toggle.click();
  const sheet = page.getByRole('dialog', { name: 'Filter options' });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole('button', { name: 'POST', exact: true })).toBeVisible();
  const behind = await page.locator('[role="option"]').first().boundingBox();
  expect(behind?.y).toBe(before);
  const focusInside = await sheet.evaluate((node) => node.contains(document.activeElement));
  expect(focusInside).toBe(true);

  await sheet.getByRole('button', { name: 'GET', exact: true }).click();
  await expect(sheet.getByRole('button', { name: 'GET', exact: true })).toHaveAttribute('aria-pressed', 'true');

  await page.keyboard.press('Escape');
  await expect(sheet).toBeHidden();
  await expect(page.getByRole('button', { name: 'Filters (1)' })).toBeFocused();
});

test('at 1440 the filter block is compact and the workspace reaches the bottom of the window', async ({ page }) => {
  await openLive(page, WIDE, longPathSeries(40));

  const filterHeight = await page.getByRole('search').evaluate((node) => node.getBoundingClientRect().height);
  expect(filterHeight).toBeLessThanOrEqual(96);

  const list = await listboxOf(page).locator('xpath=..').boundingBox();
  const pager = await page.getByRole('button', { name: 'Last' }).boundingBox();
  expect((pager?.y ?? 0) + (pager?.height ?? 0)).toBeGreaterThan(WIDE.height - 24);
  const bodyBottom = await page.getByRole('main').evaluate((node) => node.getBoundingClientRect().bottom);
  expect(bodyBottom).toBeGreaterThanOrEqual(WIDE.height - 1);
  expect(list?.height ?? 0).toBeGreaterThan(300);
  const rows = await listboxOf(page).getByRole('option').count();
  expect(rows).toBeGreaterThanOrEqual(10);
});

test('at 1440 the filter options open in a popover and give the focus back on Escape', async ({ page }) => {
  await openLive(page, WIDE, longPathSeries(10));

  const toggle = page.getByRole('button', { name: 'Filters (0)' });
  await toggle.click();
  const popover = page.getByRole('dialog', { name: 'Filter options' });
  await expect(popover).toBeVisible();
  await popover.getByRole('button', { name: 'POST', exact: true }).click();
  await expect(popover.getByRole('button', { name: 'POST', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('button', { name: 'Filters (1)' })).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(popover).toBeHidden();
  await expect(page.getByRole('button', { name: 'Filters (1)' })).toBeFocused();
});

test('an arrival stamped ahead of the browser clock reads as now, not in the future', async ({ page }) => {
  const series = longPathSeries(6);
  const state = await openLive(page, WIDE, series.slice(0, 5));
  const arrival = {
    ...(series[5] as Capture),
    received_at: new Date(Date.now() + CLOCK_AHEAD_MS).toISOString(),
  };
  state.captures.push(arrival);
  await pushStreamEvent(page, 'capture', { ...summaryOf(arrival), endpoint_id: ENDPOINT_ID }, String(arrival.seq));

  const newest = listboxOf(page).getByRole('option').first();
  await expect(newest).toContainText('/6');
  await expect(newest.locator('time')).toHaveText('now');
  await expect(listboxOf(page)).not.toContainText(/\bin \d+ seconds?\b/);
});

test('sizes read the same in the list and in the detail, in binary units', async ({ page }) => {
  const captures = captureSeries(3).map((capture) => ({
    ...capture,
    body_size: capture.seq === 1 ? 2048 : capture.seq === 2 ? 1024 * 1024 : 22,
    body: { encoding: 'utf8' as const, data: '{"a":1}', truncated: false },
  }));
  await openEndpoint(page, WIDE, captures, `/e/${ENDPOINT_ID}/c/${captureIdFor(1)}`);

  const list = listboxOf(page);
  await expect(list).toContainText('2.0 KiB');
  await expect(list).toContainText('1.0 MiB');
  await expect(list).toContainText('22 B');
  await expect(page.getByText('2.0 KiB', { exact: true }).first()).toBeVisible();
  await expect(list).not.toContainText(/\d(kB|MB)\b/);
});

test('the collapsed rail shows icons only, with accessible names, inside 56 px', async ({ page }) => {
  await page.setViewportSize(WIDE);
  await mockApi(page, endpointReplies(0));
  await page.goto(`/e/${ENDPOINT_ID}/settings/response`);

  const aside = page.locator('aside').first();
  await expect(aside).toBeVisible();
  const width = await aside.evaluate((node) => node.getBoundingClientRect().width);
  expect(width).toBeLessThanOrEqual(57);
  await expect(aside.getByRole('button', { name: 'New endpoint' })).toBeVisible();
  await expect(aside.getByRole('button', { name: 'Expand' })).toBeVisible();
  await expect(aside.getByRole('link', { name: 'Composer' })).toBeVisible();

  const clipped = await aside.evaluate((node) => {
    const bounds = node.getBoundingClientRect();
    return [...node.querySelectorAll('button, a')]
      .filter((element) => element.getClientRects().length > 0)
      .filter((element) => {
        const box = element.getBoundingClientRect();
        return box.left < bounds.left || box.right > bounds.right || element.scrollWidth > element.clientWidth + 1;
      })
      .map((element) => element.textContent);
  });
  expect(clipped).toEqual([]);
  const heading = await aside.getByRole('heading', { name: 'My endpoints' }).boundingBox();
  expect(heading?.width ?? Number.POSITIVE_INFINITY).toBeLessThanOrEqual(1);
  await expect(aside.getByRole('button', { name: 'New endpoint' })).toHaveText('+');
});

test('the header has distinct Copy URL and Copy curl buttons', async ({ page }) => {
  await openEndpoint(page, WIDE, longPathSeries(3));

  await expect(page.getByRole('button', { name: 'Copy URL', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Copy curl', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Copy', exact: true }).first()).toBeVisible();
});

test('the narrow menu is a modal drawer that takes focus and gives it back', async ({ page }) => {
  await openEndpoint(page, NARROW, longPathSeries(3));
  const menu = page.getByRole('button', { name: 'Menu' });
  await expect(menu).toBeVisible();

  await menu.click();
  const drawer = page.getByRole('dialog', { name: 'Menu' });
  await expect(drawer).toBeVisible();
  expect(await drawer.evaluate((node) => node.contains(document.activeElement))).toBe(true);
  await expect(drawer.getByRole('radio', { name: 'Dark' })).toBeVisible();
  await expect(drawer.getByRole('link', { name: 'API reference' })).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(drawer).toBeHidden();
  await expect(menu).toBeFocused();
});

test('at 360 the opened detail is its own scroller with the pager fixed to its bottom', async ({ page }) => {
  await openEndpoint(page, NARROW, longPathSeries(30), `/e/${ENDPOINT_ID}/c/${captureIdFor(5)}`);
  await expect(page.getByRole('heading', { name: /^Capture / })).toBeVisible();

  const scrolling = await page.evaluate(() => ({
    document: document.documentElement.scrollHeight - window.innerHeight,
    top: document.documentElement.scrollTop,
  }));
  expect(scrolling.document).toBeLessThanOrEqual(0);
  const pager = page.getByRole('button', { name: 'Next', exact: true });
  const pagerBox = await pager.boundingBox();
  expect((pagerBox?.y ?? 0) + (pagerBox?.height ?? 0)).toBeGreaterThan(NARROW.height - 64);
  expect((pagerBox?.y ?? 0) + (pagerBox?.height ?? 0)).toBeLessThanOrEqual(NARROW.height);

  await page.mouse.move(NARROW.width / 2, NARROW.height / 2);
  await page.mouse.wheel(0, 2000);
  expect(await page.evaluate(() => document.documentElement.scrollTop)).toBe(0);
  const afterBox = await pager.boundingBox();
  expect(afterBox?.y).toBe(pagerBox?.y);
});

test('the capture heading shows the last eight characters of the id', async ({ page }) => {
  await openEndpoint(page, WIDE, longPathSeries(3), `/e/${ENDPOINT_ID}/c/${captureIdFor(2)}`);

  await expect(page.getByRole('heading', { name: `Capture ${captureIdFor(2).slice(-8)}` })).toBeVisible();
});

test('the skip link moves focus to the capture list on an endpoint', async ({ page }) => {
  await openEndpoint(page, WIDE, longPathSeries(5));
  await expect(listboxOf(page)).toBeVisible();

  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Skip to main content' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(listboxOf(page)).toBeFocused();
});

for (const size of [WIDE, NARROW]) {
  test(`every route has its own document title (${size.width} px)`, async ({ page }) => {
    await openLive(page, size, longPathSeries(3));
    const title = 'stripe test — webhooks.live';
    await expect(page).toHaveTitle(title);

    await page.goto('/docs/api');
    await expect(page).toHaveTitle('API reference — webhooks.live');
    await page.goto('/no/such/page');
    await expect(page).toHaveTitle('Not found — webhooks.live');
  });
}

test('the endpoint list has no axe violations at 360 with the filter sheet open', async ({ page }) => {
  await openLive(page, NARROW, longPathSeries(10));
  await page.getByRole('button', { name: 'Filters (0)' }).click();
  await expect(page.getByRole('dialog', { name: 'Filter options' })).toBeVisible();

  const result = await new AxeBuilder({ page }).analyze();
  expect(result.violations).toEqual([]);
});

test('the endpoint list has no axe violations at 360 with the menu drawer open', async ({ page }) => {
  await openLive(page, NARROW, longPathSeries(10));
  await page.getByRole('button', { name: 'Menu' }).click();
  await expect(page.getByRole('dialog', { name: 'Menu' })).toBeVisible();

  const result = await new AxeBuilder({ page }).analyze();
  expect(result.violations).toEqual([]);
});
