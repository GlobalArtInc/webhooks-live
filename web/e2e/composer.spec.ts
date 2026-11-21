import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import type { SendRequest } from '../src/api/types';
import { jsonCaptureFixture } from '../src/test/fixtures/captures';
import { sendResultFixture } from '../src/test/fixtures/composer';
import { mockCaptureApi } from './support/captureApi';
import { apiError, ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const REPLAY_PATH = `/send?endpoint=${ENDPOINT_ID}&capture=${jsonCaptureFixture.id}`;
const REPLAY_BANNER = 'Replay of POST /404/orders?x=1';
const URL_LABEL = 'Target URL';
const REFUSED_TEXT = 'This address is refused. address 10.0.0.5 is private';
const PRIVATE_TARGET = 'http://10.0.0.5/hook';

async function openComposer(page: Page, path: string, extra: ReplyMap = {}): Promise<void> {
  await mockApi(page, {
    'GET /instance': instanceWith(),
    'GET /send/history': { body: { items: [], next_cursor: null } },
    ...extra,
  });
  await mockCaptureApi(page, ENDPOINT_ID, [jsonCaptureFixture]);
  await page.goto(path);
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test('a replayed capture sends its own body through the server and shows the answer', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 900 });
  await openComposer(page, REPLAY_PATH);
  const sent: SendRequest[] = [];
  await page.route('**/api/v1/send', async (route) => {
    sent.push(JSON.parse(route.request().postData() ?? '{}') as SendRequest);
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(sendResultFixture) });
  });

  await expect(page.getByText(REPLAY_BANNER)).toBeVisible();
  await page.getByLabel(URL_LABEL).fill(`https://erp.example.test${jsonCaptureFixture.path}?x=1`);
  await page.getByRole('button', { name: 'Send', exact: true }).click();

  await expect.poll(() => sent.length).toBe(1);
  expect(sent[0]?.body).toEqual({ encoding: 'utf8', data: '{"order":42}' });
  expect(sent[0]?.source_capture_id).toBe(jsonCaptureFixture.id);
  await expect(page.getByText('200', { exact: true })).toBeVisible();
});

test('a private target is refused on the URL field', async ({ page }) => {
  await openComposer(page, '/send', {
    'POST /send': apiError(422, 'validation_failed', 'url address 10.0.0.5 is private', [
      { field: 'url', code: 'target_refused', message: 'address 10.0.0.5 is private' },
    ]),
  });

  await page.getByLabel(URL_LABEL).fill(PRIVATE_TARGET);
  await page.getByRole('button', { name: 'Send', exact: true }).click();

  await expect(page.getByLabel(URL_LABEL)).toHaveAttribute('aria-invalid', 'true');
  await expect(page.getByText(REFUSED_TEXT)).toBeVisible();
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the composer with a response has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await openComposer(page, REPLAY_PATH);
    await chooseTheme(page, theme);
    await page.route('**/api/v1/send', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(sendResultFixture) }),
    );
    await page.getByLabel(URL_LABEL).fill(`https://erp.example.test${jsonCaptureFixture.path}?x=1`);
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect(page.getByText('200', { exact: true })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}

test('the composer fits a 360 px viewport without horizontal scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await openComposer(page, REPLAY_PATH);
  await expect(page.getByText(REPLAY_BANNER)).toBeVisible();

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBe(0);
});
