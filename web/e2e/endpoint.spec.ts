import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import { mockCaptureApi, type CaptureApiState } from './support/captureApi';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const LIST_NAME = 'Captures';
const MAX_MOUNTED_ROWS = 60;

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

async function openEndpoint(page: Page, captureCount: number, path: string = ENDPOINT_PATH): Promise<CaptureApiState> {
  await mockApi(page, endpointReplies(captureCount));
  const state = await mockCaptureApi(page, ENDPOINT_ID, captureSeries(captureCount));
  await page.goto(path);
  await expect(page.getByRole('listbox', { name: LIST_NAME })).toBeVisible();
  return state;
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

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

test('500 captures scroll with only a window of rows mounted and load later pages as needed', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openEndpoint(page, 500);
  const listbox = page.getByRole('listbox', { name: LIST_NAME });

  await expect(listbox.getByRole('option').first()).toHaveAttribute('aria-setsize', '500');
  expect(await listbox.getByRole('option').count()).toBeLessThan(MAX_MOUNTED_ROWS);

  const scroller = listbox.locator('xpath=..');
  await expect
    .poll(async () => {
      await scroller.evaluate((element) => {
        element.scrollTop = element.scrollHeight;
      });
      return listbox.locator('[aria-posinset="500"]').count();
    })
    .toBe(1);
  expect(await listbox.getByRole('option').count()).toBeLessThan(MAX_MOUNTED_ROWS);
  await expect(page.getByRole('button', { name: 'Load more' })).toHaveCount(0);

  await scroller.evaluate((element) => {
    element.scrollTop = element.scrollHeight / 2;
  });
  await expect(listbox.locator('[aria-posinset="250"]')).toHaveCount(1);
  expect(await listbox.getByRole('option').count()).toBeLessThan(MAX_MOUNTED_ROWS);
});

test('a deep link selects its capture, scrolls it into view and shows it in the detail pane', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openEndpoint(page, 500, `${ENDPOINT_PATH}/c/${captureIdFor(250)}`);
  const listbox = page.getByRole('listbox', { name: LIST_NAME });

  const selected = listbox.locator('[aria-selected="true"]');
  await expect(selected).toHaveAttribute('aria-posinset', '251');
  await expect(selected).toBeInViewport();
  await expect(page.getByRole('region', { name: 'Selected capture' })).toBeVisible();
  await expect(page).toHaveURL(`${ENDPOINT_PATH}/c/${captureIdFor(250)}`);
});

test('delete with Undo keeps the capture when Undo is pressed and sends the DELETE when the toast expires', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const state = await openEndpoint(page, 5);
  const listbox = page.getByRole('listbox', { name: LIST_NAME });

  await page.getByRole('button', { name: 'Delete capture' }).first().click();
  await expect(listbox.getByRole('option')).toHaveCount(4);
  await expect(page.getByText('Capture deleted')).toBeVisible();

  await page.getByRole('button', { name: 'Undo' }).click();
  await expect(listbox.getByRole('option')).toHaveCount(5);
  expect(state.deleted).toEqual([]);

  await page.getByRole('button', { name: 'Delete capture' }).first().click();
  await expect(page.getByRole('button', { name: 'Undo' })).toBeHidden({ timeout: 10_000 });
  await expect.poll(() => state.deleted.length).toBe(1);
  await expect(listbox.getByRole('option')).toHaveCount(4);
});

test('the 360 px flow lists first, opens the detail full screen, returns with Back and never scrolls sideways', async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await openEndpoint(page, 20);
  const listbox = page.getByRole('listbox', { name: LIST_NAME });

  await expect(listbox).toBeVisible();
  expect(await horizontalOverflow(page)).toBe(0);

  await listbox.getByRole('option').first().click();
  await expect(page).toHaveURL(new RegExp(`${ENDPOINT_PATH}/c/[0-9a-f-]{36}$`));
  const detail = page.getByRole('region', { name: 'Selected capture' });
  await expect(detail).toBeVisible();
  await expect(page.getByRole('button', { name: 'Back' })).toBeVisible();
  const box = await detail.boundingBox();
  expect(box?.width).toBe(360);
  expect(await horizontalOverflow(page)).toBe(0);

  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page).toHaveURL(ENDPOINT_PATH);
  await expect(listbox).toBeVisible();
  expect(await horizontalOverflow(page)).toBe(0);
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the endpoint view has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openEndpoint(page, 5, `${ENDPOINT_PATH}/c/${captureIdFor(3)}`);
    await chooseTheme(page, theme);
    await expect(page.getByRole('region', { name: 'Selected capture' })).toBeVisible();

    const populated = await new AxeBuilder({ page }).analyze();
    expect(populated.violations).toEqual([]);
  });

  test(`the empty endpoint has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mockApi(page, endpointReplies(0));
    await mockCaptureApi(page, ENDPOINT_ID, []);
    await page.goto(ENDPOINT_PATH);
    await chooseTheme(page, theme);
    await expect(page.getByText('Waiting for the first request')).toBeVisible();

    const empty = await new AxeBuilder({ page }).analyze();
    expect(empty.violations).toEqual([]);
  });
}
