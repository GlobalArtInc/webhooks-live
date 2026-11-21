import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { instanceFixture, PUBLIC_URL } from '../src/test/fixtures/instance';
import {
  pausedScheduleFixture,
  SCHEDULE_ID,
  scheduleFixture,
  scheduleRunFixtures,
} from '../src/test/fixtures/schedules';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const BASE_PATH = `/e/${ENDPOINT_ID}`;
const SECTION_PATH = `${BASE_PATH}/settings/schedules`;
const SCHEDULES_PATH = `/endpoints/${ENDPOINT_ID}/schedules`;
const SCHEDULE_PATH = `${SCHEDULES_PATH}/${SCHEDULE_ID}`;
const LIST_NAME = 'Schedules';
const WIDTHS = [
  { width: 360, height: 740 },
  { width: 1024, height: 768 },
  { width: 1440, height: 900 },
] as const;

function scheduleReplies(extra: ReplyMap = {}): ReplyMap {
  return {
    'GET /instance': instanceWith({ features: { ...instanceFixture.features, schedules: true } }),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}`, my_role: 'owner' },
    },
    [`GET ${SCHEDULES_PATH}`]: { body: { items: [scheduleFixture, pausedScheduleFixture] } },
    [`GET ${SCHEDULE_PATH}/runs`]: { body: { items: scheduleRunFixtures } },
    ...extra,
  };
}

async function openSchedules(page: Page, path: string, extra: ReplyMap = {}): Promise<void> {
  await mockApi(page, scheduleReplies(extra));
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
  test(`the schedules section lists schedules without sideways scrolling at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openSchedules(page, SECTION_PATH);

    await expect(page.getByRole('heading', { level: 2, name: 'Schedules' })).toBeVisible();
    await expect(page.getByRole('list', { name: LIST_NAME })).toBeVisible();
    await expect(page.getByRole('list', { name: LIST_NAME }).getByText('Paused')).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);
  });
}

test('an invalid cron expression lands on the cadence field and blocks the save', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSchedules(page, SECTION_PATH);

  await page.getByRole('button', { name: 'New schedule', exact: true }).click();
  await page.getByRole('textbox', { name: 'Name', exact: true }).fill('Weekly ping');
  await page.getByRole('textbox', { name: 'URL', exact: true }).fill('https://staging.example.test/weekly');
  await page.getByRole('radio', { name: 'Cron expression', exact: true }).check();
  await page
    .getByRole('textbox', { name: 'Expression, five fields: minute hour day month weekday', exact: true })
    .fill('every five minutes');
  await page.getByRole('button', { name: 'Save', exact: true }).click();

  const expression = page.getByRole('textbox', {
    name: 'Expression, five fields: minute hour day month weekday',
    exact: true,
  });
  await expect(expression).toHaveAttribute('aria-invalid', 'true');
  await expect(page.getByText('Not a valid five-field cron expression.')).toBeVisible();
  await expect(page.getByText('Enter a valid cadence to see the next runs.')).toBeVisible();
});

test('pausing a schedule sends one update to the server', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSchedules(page, SECTION_PATH, {
    [`PATCH ${SCHEDULE_PATH}`]: { body: { ...scheduleFixture, enabled: false } },
  });

  await page.getByRole('button', { name: /Ping staging/ }).click();
  const update = page.waitForRequest(
    (request) => request.method() === 'PATCH' && request.url().endsWith(SCHEDULE_PATH),
  );
  await page.getByRole('switch', { name: 'Schedule active' }).click();

  expect((await update).postDataJSON()).toEqual({ enabled: false });
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the schedules section has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openSchedules(page, SECTION_PATH);
    await page.getByRole('button', { name: /Ping staging/ }).click();
    await chooseTheme(page, theme);
    await expect(page.getByRole('heading', { level: 3, name: 'Ping staging' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
