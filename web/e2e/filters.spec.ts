import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import type { Capture } from '../src/api/types';
import { captureSeries } from '../src/features/endpoint/testFixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { summaryOf } from '../src/test/msw/store';
import { mockCaptureApi, type CaptureApiState } from './support/captureApi';
import { installFakeStream, openLatestStream, pushStreamEvent, streamCount } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const LIST_NAME = 'Captures';
const SEARCH_NAME = 'Search captures';
const WIDE = { width: 1440, height: 900 };
const NARROW = { width: 360, height: 800 };

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

function methodSeries(count: number): Capture[] {
  return captureSeries(count).map((capture) => ({ ...capture, method: capture.seq % 2 === 1 ? 'GET' : 'POST' }));
}

function listboxOf(page: Page): Locator {
  return page.getByRole('listbox', { name: LIST_NAME });
}

async function openFilteredEndpoint(page: Page, initial: Capture[], path: string): Promise<CaptureApiState> {
  await installFakeStream(page);
  await mockApi(page, endpointReplies(initial.length));
  const state = await mockCaptureApi(page, ENDPOINT_ID, initial);
  await page.goto(path);
  await expect(page.getByRole('search')).toBeVisible();
  await expect.poll(() => streamCount(page)).toBeGreaterThan(0);
  await openLatestStream(page);
  await expect(page.getByText('Live', { exact: true })).toBeVisible();
  return state;
}

async function openFilterOptions(page: Page, count: number): Promise<void> {
  await page.getByRole('button', { name: `Filters (${count})` }).click();
  await expect(page.getByRole('dialog', { name: 'Filter options' })).toBeVisible();
}

async function arrive(page: Page, state: CaptureApiState, capture: Capture): Promise<void> {
  state.captures.push(capture);
  await pushStreamEvent(page, 'capture', { ...summaryOf(capture), endpoint_id: ENDPOINT_ID }, String(capture.seq));
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document.getAnimations().filter((animation) => animation.effect?.getComputedTiming().endTime !== Infinity)
            .length,
      ),
    )
    .toBe(0);
}

test('a filtered link reopens the same list with its chips pressed', async ({ page }) => {
  await page.setViewportSize(WIDE);
  const state = await openFilteredEndpoint(page, methodSeries(6), `${ENDPOINT_PATH}?method=GET`);

  await expect(listboxOf(page).getByRole('option')).toHaveCount(3);
  await openFilterOptions(page, 1);
  await expect(page.getByRole('button', { name: 'GET', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('button', { name: 'POST', exact: true })).toHaveAttribute('aria-pressed', 'false');
  expect(state.listRequests.some((search) => search.includes('method=GET'))).toBe(true);
});

test('typing a text query sends one request after the pause and shows its match', async ({ page }) => {
  await page.setViewportSize(WIDE);
  const state = await openFilteredEndpoint(page, methodSeries(6), ENDPOINT_PATH);

  await page.getByRole('searchbox', { name: SEARCH_NAME }).pressSequentially('hook/4');

  await expect(listboxOf(page).getByRole('option')).toHaveCount(1);
  expect(state.listRequests.filter((search) => search.includes('q='))).toHaveLength(1);
});

test('arrivals that do not match are counted instead of inserted, and Show all brings them back', async ({ page }) => {
  await page.setViewportSize(WIDE);
  const series = methodSeries(7);
  const state = await openFilteredEndpoint(page, series.slice(0, 5), `${ENDPOINT_PATH}?method=GET`);
  await expect(listboxOf(page).getByRole('option')).toHaveCount(3);

  await arrive(page, state, series[5] as Capture);
  await expect(page.getByText('1 new capture hidden by filters')).toBeVisible();
  await expect(listboxOf(page).getByRole('option')).toHaveCount(3);

  await arrive(page, state, series[6] as Capture);
  await expect(listboxOf(page).getByRole('option')).toHaveCount(4);
  await expect(page.getByText('1 new capture hidden by filters')).toBeVisible();

  await page.getByRole('button', { name: 'Show all captures' }).click();
  await expect(listboxOf(page).getByRole('option')).toHaveCount(7);
  await expect(page.getByText('hidden by filters')).toHaveCount(0);
});

test('the narrow layout keeps the filter options in a sheet behind the Filters button', async ({ page }) => {
  await page.setViewportSize(NARROW);
  await openFilteredEndpoint(page, methodSeries(6), `${ENDPOINT_PATH}?method=GET`);

  const toggle = page.getByRole('button', { name: 'Filters (1)' });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByRole('button', { name: 'POST', exact: true })).toBeHidden();

  await toggle.click();
  await expect(page.getByRole('dialog', { name: 'Filter options' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'GET', exact: true })).toHaveAttribute('aria-pressed', 'true');
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the filtered endpoint view has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize(WIDE);
    await openFilteredEndpoint(page, methodSeries(6), `${ENDPOINT_PATH}?method=GET&q=hook`);
    await chooseTheme(page, theme);
    await expect(listboxOf(page).getByRole('option')).toHaveCount(3);

    const result = await new AxeBuilder({ page }).analyze();
    expect(result.violations).toEqual([]);
  });
}
