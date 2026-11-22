import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import type { Capture, Endpoint } from '../src/api/types';
import { jsonCaptureFixture } from '../src/test/fixtures/captures';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import {
  dnsCaptureFixture,
  DNS_NAME,
  emailCaptureFixture,
  mailEndpointFixture,
  scheduleCaptureFixture,
} from '../src/test/fixtures/kinds';
import { mockCaptureApi, type CaptureApiState } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const LIST_NAME = 'Captures';
const WIDE = { width: 1440, height: 900 };
const NARROW = { width: 360, height: 800 };
const MIXED: Capture[] = [jsonCaptureFixture, emailCaptureFixture, dnsCaptureFixture, scheduleCaptureFixture];

function endpointWith(captureCount: number): Endpoint {
  return {
    ...mailEndpointFixture,
    id: ENDPOINT_ID,
    url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
    capture_count: captureCount,
    last_seq: captureCount,
  };
}

function replies(captureCount: number): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: endpointWith(captureCount) },
  };
}

function listboxOf(page: Page): Locator {
  return page.getByRole('listbox', { name: LIST_NAME });
}

async function openEndpoint(page: Page, path: string, captures: Capture[]): Promise<CaptureApiState> {
  await installFakeStream(page);
  await mockApi(page, replies(captures.length));
  const state = await mockCaptureApi(page, ENDPOINT_ID, captures);
  await page.goto(path);
  await expect(listboxOf(page)).toBeVisible();
  return state;
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test('mail, DNS and scheduled captures show their own badges and lines in the wide layout', async ({ page }) => {
  await page.setViewportSize(WIDE);
  await openEndpoint(page, ENDPOINT_PATH, MIXED);

  await expect(listboxOf(page).getByRole('option')).toHaveCount(4);
  const rows = listboxOf(page).getByRole('option');
  await expect(rows.filter({ hasText: 'Invoice 2026-11' }).getByText('MAIL', { exact: true })).toBeVisible();
  await expect(rows.filter({ hasText: `x1.${DNS_NAME}` }).getByText('DNS', { exact: true })).toBeVisible();
  await expect(rows.filter({ hasText: 'Scheduled' })).toHaveCount(1);
});

test('the kind chips send the kind to the list query and show only the chosen kind', async ({ page }) => {
  await page.setViewportSize(WIDE);
  const state = await openEndpoint(page, ENDPOINT_PATH, MIXED);

  await page.getByRole('button', { name: 'Filters (0)' }).click();
  const email = page
    .getByRole('dialog', { name: 'Filter options' })
    .getByRole('button', { name: 'Email', exact: true });
  await email.click();
  await expect(email).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(() => state.listRequests.some((search) => search.includes('kind=email'))).toBe(true);
  await expect(listboxOf(page).getByRole('option')).toHaveCount(1);
  await expect(listboxOf(page).getByRole('option').filter({ hasText: 'Invoice 2026-11' })).toHaveCount(1);
});

test('the narrow layout lists DNS captures with their query name', async ({ page }) => {
  await page.setViewportSize(NARROW);
  await openEndpoint(page, `${ENDPOINT_PATH}?kind=dns`, MIXED);

  await expect(listboxOf(page).getByRole('option')).toHaveCount(1);
  await expect(
    listboxOf(page)
      .getByRole('option')
      .filter({ hasText: `x1.${DNS_NAME}` }),
  ).toHaveCount(1);
});

test('the DNS capture detail shows the query name, label, type, asking address and flags', async ({ page }) => {
  await page.setViewportSize(WIDE);
  await openEndpoint(page, ENDPOINT_PATH, MIXED);

  await listboxOf(page)
    .getByRole('option')
    .filter({ hasText: `x1.${DNS_NAME}` })
    .click();
  await expect(page).toHaveURL(new RegExp(`${ENDPOINT_PATH}/c/`));
  const detail = page.getByRole('region', { name: 'Selected capture' });
  const group = detail.getByRole('group', { name: 'DNS query' });
  await expect(group).toBeVisible();
  await expect(group.getByText(`x1.${DNS_NAME}`, { exact: true })).toBeVisible();
  await expect(group.getByText('x1', { exact: true })).toBeVisible();
  await expect(group.getByText('A (1)', { exact: true })).toBeVisible();
  await expect(group.getByText('198.51.100.53', { exact: true })).toBeVisible();
  await expect(group.getByText('RD, recursion desired; DO, DNSSEC OK', { exact: true })).toBeVisible();
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the mixed-kind endpoint view has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize(WIDE);
    await openEndpoint(page, `${ENDPOINT_PATH}?kind=email,dns`, MIXED);
    await chooseTheme(page, theme);
    await expect(listboxOf(page).getByRole('option')).toHaveCount(2);

    const result = await new AxeBuilder({ page }).analyze();
    expect(result.violations).toEqual([]);
  });
}
