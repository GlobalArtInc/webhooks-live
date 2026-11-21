import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { SIGNED_IN_USER } from '../src/features/tokens/testFixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/cli`;
const ENDPOINT_URL = `${PUBLIC_URL}/${ENDPOINT_ID}`;
const COMMAND_REGION = 'Command';
const COPY_COMMAND = 'Copy the command';
const TOKENS_LINK = 'Create a token on the API tokens page';
const PORT_ERROR = 'Enter a port from 1 to 65535.';

function endpointReplies(extra: ReplyMap = {}): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: ENDPOINT_URL },
    },
    ...extra,
  };
}

async function openSection(page: Page, extra: ReplyMap = {}): Promise<void> {
  await mockApi(page, endpointReplies(extra));
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await installFakeStream(page);
  await page.goto(SECTION_PATH);
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

test('the section offers the install tabs and builds the command from the capture URL', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSection(page);

  await expect(page.getByRole('heading', { level: 2, name: 'Local forwarding' })).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Linux x86_64' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('link', { name: 'Download the archive' })).toHaveAttribute(
    'href',
    /\/releases\/download\/cli-v0\.1\.0-dev\/webhooks-live-cli-0\.1\.0-dev-linux-amd64\.tar\.gz$/,
  );
  await expect(page.getByRole('region', { name: COMMAND_REGION })).toHaveText(
    `webhooks-live-cli forward ${ENDPOINT_URL} --to http://localhost:3000`,
  );
});

test('the install tabs move with the arrow keys and keep focus on the selected tab', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSection(page);

  await page.getByRole('tab', { name: 'Linux x86_64' }).focus();
  await page.keyboard.press('ArrowRight');

  await expect(page.getByRole('tab', { name: 'Linux arm64' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('tab', { name: 'Linux arm64' })).toBeFocused();
});

test('the copied command carries the port, prefix, start point and dry run flag', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSection(page);

  await page.getByLabel('Local port').fill('8080');
  await page.getByLabel('Path prefix on the local server').fill('/hooks');
  await page.getByRole('radio', { name: 'The whole history that the server still holds' }).check();
  await page.getByRole('checkbox', { name: 'Dry run: print what would be sent, send nothing' }).check();

  const expected = `webhooks-live-cli forward ${ENDPOINT_URL} --to http://localhost:8080/hooks --from all --dry-run`;
  await expect(page.getByRole('region', { name: COMMAND_REGION })).toHaveText(expected);
  await page.getByRole('button', { name: COPY_COMMAND }).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(expected);
});

test('an invalid port shows the error and withholds the command', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSection(page);

  await page.getByLabel('Local port').fill('0');

  await expect(page.getByLabel('Local port')).toHaveAttribute('aria-invalid', 'true');
  await expect(page.getByText(PORT_ERROR)).toBeVisible();
  await expect(page.getByRole('region', { name: COMMAND_REGION })).toHaveCount(0);
});

test('the token step links to the API tokens page and the command never names a token', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSection(page, { 'GET /instance': instanceWith({ auth_mode: 'sso', user: SIGNED_IN_USER }) });

  await expect(page.getByRole('link', { name: TOKENS_LINK })).toHaveAttribute('href', '/settings/tokens');
  await expect(page.getByRole('region', { name: COMMAND_REGION })).not.toContainText('WEBHOOKS_LIVE_TOKEN');
});

test('a long path prefix scrolls inside the command block at 360 px', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await openSection(page);

  await page.getByLabel('Path prefix on the local server').fill(`/${'segment-'.repeat(12)}`);

  await expect(page.getByRole('region', { name: COMMAND_REGION })).toBeVisible();
  await expect.poll(() => horizontalOverflow(page)).toBe(0);
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the local forwarding section has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openSection(page);
    await chooseTheme(page, theme);
    await expect(page.getByRole('heading', { level: 2, name: 'Local forwarding' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
