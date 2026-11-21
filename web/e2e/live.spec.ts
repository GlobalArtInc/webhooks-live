import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import type { Capture, CaptureSummary } from '../src/api/types';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { summaryOf } from '../src/test/msw/store';
import { mockCaptureApi, type CaptureApiState } from './support/captureApi';
import {
  failLatestStream,
  installFakeStream,
  isLatestStreamClosed,
  latestStreamUrl,
  openLatestStream,
  pushStreamEvent,
  setTabHidden,
  streamCount,
} from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const LIST_NAME = 'Captures';
const ROW_HEIGHT_PX = 36;
const SERIES_LENGTH = 200;
const BASE_TITLE = 'webhooks.live';
const ORIGINAL_ICON = '/favicon.svg';

interface LiveRig {
  state: CaptureApiState;
  series: Capture[];
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

function listboxOf(page: Page): Locator {
  return page.getByRole('listbox', { name: LIST_NAME });
}

function scrollerOf(page: Page): Locator {
  return listboxOf(page).locator('xpath=..');
}

async function openLive(page: Page, count: number, path: string = ENDPOINT_PATH): Promise<LiveRig> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeStream(page);
  await mockApi(page, endpointReplies(count));
  const series = captureSeries(SERIES_LENGTH);
  const state = await mockCaptureApi(page, ENDPOINT_ID, series.slice(0, count));
  await page.goto(path);
  await expect(listboxOf(page)).toBeVisible();
  await expect.poll(() => streamCount(page)).toBeGreaterThan(0);
  await openLatestStream(page);
  await expect(page.getByText('Live', { exact: true })).toBeVisible();
  return { state, series };
}

function captureAt(rig: LiveRig, seq: number): Capture {
  const capture = rig.series[seq - 1];
  if (capture === undefined) {
    throw new Error(`no fixture capture with seq ${seq}`);
  }
  return capture;
}

function summaryAt(rig: LiveRig, seq: number): CaptureSummary {
  return { ...summaryOf(captureAt(rig, seq)), endpoint_id: ENDPOINT_ID };
}

async function arrive(page: Page, rig: LiveRig, seq: number): Promise<void> {
  rig.state.captures.push(captureAt(rig, seq));
  await pushStreamEvent(page, 'capture', summaryAt(rig, seq), String(seq));
}

async function optionPaths(page: Page): Promise<number[]> {
  const texts = await listboxOf(page).locator('[title^="/hook/"]').allTextContents();
  return texts.map((text) => Number(/^\/hook\/(\d+)$/.exec(text)?.[1] ?? Number.NaN));
}

async function scrollTop(page: Page): Promise<number> {
  return scrollerOf(page).evaluate((element) => element.scrollTop);
}

async function scrollListTo(page: Page, top: number): Promise<void> {
  await scrollerOf(page).evaluate(async (element, offset) => {
    element.scrollTop = offset;
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          resolve();
        });
      });
    });
  }, top);
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test('a request appears live without a reload', async ({ page }) => {
  const rig = await openLive(page, 5);
  await page.evaluate(() => {
    Object.defineProperty(window, '__wlMarker', { value: 'kept' });
  });

  await arrive(page, rig, 6);

  const newest = listboxOf(page).getByRole('option').first();
  await expect(newest).toContainText('/hook/6');
  await expect(newest).toHaveAttribute('aria-posinset', '1');
  await expect(newest).toHaveAttribute('aria-setsize', '6');
  await expect(page.getByText('6 / 500 captures')).toBeVisible();
  expect(await page.evaluate(() => Reflect.get(window, '__wlMarker'))).toBe('kept');
});

test('the selection stays on the capture it was on when new captures arrive', async ({ page }) => {
  const rig = await openLive(page, 5, `${ENDPOINT_PATH}/c/${captureIdFor(3)}`);
  const listbox = listboxOf(page);
  await expect(listbox.locator('[aria-selected="true"]')).toContainText('/hook/3');

  await arrive(page, rig, 6);
  await arrive(page, rig, 7);

  await expect(listbox.getByRole('option').first()).toContainText('/hook/7');
  await expect(listbox.locator('[aria-selected="true"]')).toContainText('/hook/3');
  await expect(page).toHaveURL(`${ENDPOINT_PATH}/c/${captureIdFor(3)}`);
});

test('new captures neither shift the viewport nor take focus while the list is scrolled away', async ({ page }) => {
  const rig = await openLive(page, 100);
  const listbox = listboxOf(page);
  await listbox.focus();
  await scrollListTo(page, 1000);
  const anchor = listbox.locator('[aria-posinset="30"]');
  await expect(anchor).toBeVisible();
  const anchorId = await anchor.getAttribute('id');
  const before = await anchor.boundingBox();
  const activeBefore = await listbox.getAttribute('aria-activedescendant');
  const scrollBefore = await scrollTop(page);

  await arrive(page, rig, 101);
  await arrive(page, rig, 102);
  await arrive(page, rig, 103);

  const pill = page.getByRole('button', { name: '3 new captures, scroll to the top' });
  await expect(pill).toBeVisible();
  const same = page.locator(`[id="${anchorId}"]`);
  await expect(same).toHaveAttribute('aria-posinset', '33');
  const after = await same.boundingBox();
  expect(Math.abs((after?.y ?? Number.NaN) - (before?.y ?? Number.NaN))).toBeLessThanOrEqual(1);
  expect(await scrollTop(page)).toBe(scrollBefore + 3 * ROW_HEIGHT_PX);
  expect(await listbox.getAttribute('aria-activedescendant')).toBe(activeBefore);
  await expect(listbox).toBeFocused();

  await arrive(page, rig, 104);
  await expect(page.getByRole('button', { name: '4 new captures, scroll to the top' })).toBeVisible();
  expect(Math.abs(((await same.boundingBox())?.y ?? Number.NaN) - (before?.y ?? Number.NaN))).toBeLessThanOrEqual(1);
});

test('the new pill scrolls to the top, clears itself and hands focus back to the list', async ({ page }) => {
  const rig = await openLive(page, 100);
  await scrollListTo(page, 800);
  await expect(listboxOf(page).locator('[aria-posinset="25"]')).toBeVisible();
  await arrive(page, rig, 101);
  await arrive(page, rig, 102);

  await page.getByRole('button', { name: '2 new captures, scroll to the top' }).click();

  await expect.poll(() => scrollTop(page)).toBe(0);
  await expect(page.getByRole('button', { name: /scroll to the top/ })).toBeHidden();
  await expect(listboxOf(page).getByRole('option').first()).toContainText('/hook/102');
  await expect(listboxOf(page)).toBeFocused();
});

test('no pill appears while the list is at the top', async ({ page }) => {
  const rig = await openLive(page, 10);

  await arrive(page, rig, 11);

  await expect(listboxOf(page).getByRole('option').first()).toContainText('/hook/11');
  await expect(page.getByRole('button', { name: /scroll to the top/ })).toHaveCount(0);
});

test('follow mode selects each arrival, pauses on an older selection and resumes on request', async ({ page }) => {
  const rig = await openLive(page, 5, `${ENDPOINT_PATH}/c/${captureIdFor(5)}`);
  const follow = page.getByRole('button', { name: 'Follow', exact: true });
  await expect(follow).toHaveAttribute('aria-pressed', 'false');
  await follow.click();
  await expect(follow).toHaveAttribute('aria-pressed', 'true');

  await arrive(page, rig, 6);
  await expect(page).toHaveURL(`${ENDPOINT_PATH}/c/${captureIdFor(6)}`);
  await expect(listboxOf(page).locator('[aria-selected="true"]')).toContainText('/hook/6');

  await listboxOf(page).getByRole('option').filter({ hasText: '/hook/3' }).click();
  await expect(page).toHaveURL(`${ENDPOINT_PATH}/c/${captureIdFor(3)}`);
  const paused = page.getByRole('button', { name: 'Follow paused' });
  await expect(paused).toHaveAttribute('aria-pressed', 'false');

  await arrive(page, rig, 7);
  await expect(listboxOf(page).getByRole('option').first()).toContainText('/hook/7');
  await expect(page).toHaveURL(`${ENDPOINT_PATH}/c/${captureIdFor(3)}`);

  await paused.click();
  await expect(page).toHaveURL(`${ENDPOINT_PATH}/c/${captureIdFor(7)}`);
  await arrive(page, rig, 8);
  await expect(page).toHaveURL(`${ENDPOINT_PATH}/c/${captureIdFor(8)}`);
});

test('follow mode is off by default and is remembered after a reload', async ({ page }) => {
  await openLive(page, 3);
  const follow = page.getByRole('button', { name: 'Follow', exact: true });
  await expect(follow).toHaveAttribute('aria-pressed', 'false');
  await follow.click();

  await page.reload();

  await expect(page.getByRole('button', { name: 'Follow', exact: true })).toHaveAttribute('aria-pressed', 'true');
});

test('a dropped stream shows reconnecting and catches up without duplicates or gaps', async ({ page }) => {
  const rig = await openLive(page, 5);
  const opened = await streamCount(page);
  await failLatestStream(page);
  await expect(page.getByText('Reconnecting...')).toBeVisible();
  for (const seq of [6, 7, 8]) {
    rig.state.captures.push(captureAt(rig, seq));
  }

  await expect.poll(() => streamCount(page), { timeout: 10_000 }).toBe(opened + 1);
  expect(await latestStreamUrl(page)).toBe(`/api/v1/endpoints/${ENDPOINT_ID}/stream?last_event_id=5`);
  await openLatestStream(page);
  await pushStreamEvent(page, 'ready', { endpoint_id: ENDPOINT_ID, last_seq: 8, capture_count: 8 });
  await pushStreamEvent(page, 'capture', summaryAt(rig, 7), '7');
  await pushStreamEvent(page, 'capture', summaryAt(rig, 8), '8');

  await expect(listboxOf(page).getByRole('option')).toHaveCount(8);
  expect(await optionPaths(page)).toEqual([8, 7, 6, 5, 4, 3, 2, 1]);
  await expect(page.getByText('Live', { exact: true })).toBeVisible();
  expect(rig.state.listRequests.filter((search) => search.includes('order=desc'))).toHaveLength(1);
  expect(rig.state.listRequests.some((search) => search.includes('after_seq=5') && search.includes('order=asc'))).toBe(
    true,
  );
});

test('a resync event lists again after the highest seq and merges by id', async ({ page }) => {
  const rig = await openLive(page, 5);
  await pushStreamEvent(page, 'ready', { endpoint_id: ENDPOINT_ID, last_seq: 5, capture_count: 5 });
  await expect.poll(() => rig.state.listRequests.length).toBeGreaterThan(1);
  rig.state.captures.push(captureAt(rig, 6), captureAt(rig, 7));
  await pushStreamEvent(page, 'capture', summaryAt(rig, 7), '7');
  await expect(listboxOf(page).getByRole('option')).toHaveCount(6);

  await pushStreamEvent(page, 'resync', { reason: 'lagged' });

  await expect(listboxOf(page).getByRole('option')).toHaveCount(7);
  expect(await optionPaths(page)).toEqual([7, 6, 5, 4, 3, 2, 1]);
});

test('a deleted capture leaves the list and an endpoint deletion shows the gone page', async ({ page }) => {
  await openLive(page, 4);

  await pushStreamEvent(page, 'capture_deleted', { capture_id: captureIdFor(2), seq: 2 });
  await expect(listboxOf(page).getByRole('option')).toHaveCount(3);

  await pushStreamEvent(page, 'endpoint_deleted', {});
  await expect(page.getByRole('heading', { name: 'This endpoint is gone' })).toBeVisible();
});

test('the title and the favicon carry the unread count while the tab is hidden', async ({ page }) => {
  const rig = await openLive(page, 3);
  await expect(page).toHaveTitle(BASE_TITLE);
  await setTabHidden(page, true);

  await arrive(page, rig, 4);
  await arrive(page, rig, 5);

  await expect(page).toHaveTitle(`(2) ${BASE_TITLE}`);
  await expect(page.locator('link[rel~="icon"]')).toHaveAttribute('href', /^data:image\/png/);

  await setTabHidden(page, false);

  await expect(page).toHaveTitle(BASE_TITLE);
  await expect(page.locator('link[rel~="icon"]')).toHaveAttribute('href', ORIGINAL_ICON);
});

test('the stream closes after a minute in a hidden tab and reconnects with a catch-up on return', async ({ page }) => {
  await page.clock.install();
  const rig = await openLive(page, 3);
  const opened = await streamCount(page);
  await setTabHidden(page, true);

  await page.clock.fastForward(61_000);

  await expect.poll(() => isLatestStreamClosed(page)).toBe(true);
  await expect(page.getByText('Offline', { exact: true })).toBeVisible();
  rig.state.captures.push(captureAt(rig, 4));

  await setTabHidden(page, false);

  await expect.poll(() => streamCount(page)).toBe(opened + 1);
  expect(await latestStreamUrl(page)).toBe(`/api/v1/endpoints/${ENDPOINT_ID}/stream?last_event_id=3`);
  await openLatestStream(page);
  await pushStreamEvent(page, 'ready', { endpoint_id: ENDPOINT_ID, last_seq: 4, capture_count: 4 });
  await expect(listboxOf(page).getByRole('option').first()).toContainText('/hook/4');
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the live layer has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    const rig = await openLive(page, 100);
    await chooseTheme(page, theme);
    await scrollListTo(page, 800);
    await expect(listboxOf(page).locator('[aria-posinset="25"]')).toBeVisible();
    await arrive(page, rig, 101);
    await expect(page.getByRole('button', { name: '1 new capture, scroll to the top' })).toBeVisible();
    await page.getByRole('button', { name: 'Follow', exact: true }).click();

    const result = await new AxeBuilder({ page }).analyze();

    expect(result.violations).toEqual([]);
  });
}
