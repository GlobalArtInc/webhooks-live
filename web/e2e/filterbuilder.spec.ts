import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import type { Capture } from '../src/api/types';
import { captureSeries } from '../src/features/endpoint/testFixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream, openLatestStream, streamCount } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const TRIGGER_NAME = 'Build filter';
const BUILDER_NAME = 'Filter builder';
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
  return page.getByRole('listbox', { name: 'Captures' });
}

async function openEndpoint(page: Page, captures: Capture[], path: string): Promise<void> {
  await installFakeStream(page);
  await mockApi(page, endpointReplies(captures.length));
  await mockCaptureApi(page, ENDPOINT_ID, captures);
  await page.goto(path);
  await expect(page.getByRole('search')).toBeVisible();
  await expect.poll(() => streamCount(page)).toBeGreaterThan(0);
  await openLatestStream(page);
  await expect(page.getByText('Live', { exact: true })).toBeVisible();
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

test('a filter built in the popover goes into the URL and narrows the list', async ({ page }) => {
  await page.setViewportSize(WIDE);
  await openEndpoint(page, methodSeries(6), ENDPOINT_PATH);

  await page.getByRole('button', { name: TRIGGER_NAME, exact: true }).click();
  const dialog = page.getByRole('dialog', { name: BUILDER_NAME });
  await dialog.getByRole('checkbox', { name: 'POST', exact: true }).check();
  await dialog.getByRole('button', { name: 'Apply', exact: true }).click();

  await expect(page).toHaveURL(/method=POST/);
  await expect(listboxOf(page).getByRole('option')).toHaveCount(3);
  await expect(page.getByRole('dialog')).toBeHidden();
});

test('a saved filter survives a reload and reopens the same result from its URL', async ({ page }) => {
  await page.setViewportSize(WIDE);
  await openEndpoint(page, methodSeries(6), ENDPOINT_PATH);

  await page.getByRole('button', { name: TRIGGER_NAME, exact: true }).click();
  const dialog = page.getByRole('dialog', { name: BUILDER_NAME });
  await dialog.getByRole('checkbox', { name: 'POST', exact: true }).check();
  await dialog.getByRole('textbox', { name: 'Name for this filter' }).fill('POST only');
  await dialog.getByRole('button', { name: 'Save filter', exact: true }).click();
  await expect(dialog.getByRole('button', { name: 'Apply POST only', exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toBeHidden();

  await page.reload();
  await expect(page.getByRole('search')).toBeVisible();
  await expect(listboxOf(page).getByRole('option')).toHaveCount(6);
  await page.getByRole('button', { name: TRIGGER_NAME, exact: true }).click();
  await page
    .getByRole('dialog', { name: BUILDER_NAME })
    .getByRole('button', { name: 'Apply POST only', exact: true })
    .click();

  await expect(page).toHaveURL(/method=POST/);
  await expect(listboxOf(page).getByRole('option')).toHaveCount(3);
});

test('the narrow layout opens the builder as a bottom sheet from the keyboard and Escape returns focus', async ({
  page,
}) => {
  await page.setViewportSize(NARROW);
  await openEndpoint(page, methodSeries(6), ENDPOINT_PATH);

  const trigger = page.getByRole('button', { name: TRIGGER_NAME, exact: true });
  await trigger.focus();
  await page.keyboard.press('Enter');
  const sheet = page.getByRole('dialog', { name: BUILDER_NAME });
  await expect(sheet).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(sheet).toBeHidden();
  await expect(trigger).toBeFocused();
});

test('an invalid sender range shows its message and leaves the URL unchanged', async ({ page }) => {
  await page.setViewportSize(WIDE);
  await openEndpoint(page, methodSeries(6), ENDPOINT_PATH);

  await page.getByRole('button', { name: TRIGGER_NAME, exact: true }).click();
  const dialog = page.getByRole('dialog', { name: BUILDER_NAME });
  await dialog.getByRole('textbox', { name: 'Sender address or IPv4 range' }).fill('203.0.113.0/40');
  await dialog.getByRole('button', { name: 'Apply', exact: true }).click();

  await expect(dialog.getByRole('alert')).toContainText('Enter an address such as 203.0.113.7');
  await expect(page).not.toHaveURL(/ip=/);
});

test('a sender filter narrows the loaded captures and labels the count as client-side', async ({ page }) => {
  await page.setViewportSize(WIDE);
  const captures = methodSeries(6).map((capture, index) => ({
    ...capture,
    client_ip: index % 2 === 0 ? '203.0.113.7' : '198.51.100.4',
  }));
  await openEndpoint(page, captures, `${ENDPOINT_PATH}?ip=203.0.113.7`);

  await expect(listboxOf(page).getByRole('option')).toHaveCount(3);
  await expect(page.getByText('3 shown (client filter)')).toBeVisible();
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the open filter builder has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize(WIDE);
    await openEndpoint(page, methodSeries(6), ENDPOINT_PATH);
    await chooseTheme(page, theme);

    await page.getByRole('button', { name: TRIGGER_NAME, exact: true }).click();
    await expect(page.getByRole('dialog', { name: BUILDER_NAME })).toBeVisible();

    const result = await new AxeBuilder({ page }).analyze();
    expect(result.violations).toEqual([]);
  });
}
