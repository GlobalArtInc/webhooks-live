import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page, type Route } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { EXPORT_TITLE, jsonExportText } from '../src/test/fixtures/exports';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const IMPORT_INTO_PATH = `/api/v1/endpoints/${ENDPOINT_ID}/import`;
const IMPORT_NEW_PATH = '/api/v1/endpoints/import';
const DIALOG_NAME = 'Import captures';
const ZIP_MESSAGE = 'ZIP files cannot be imported. Export the endpoint as JSON or NDJSON and upload that file.';
const BUSY_MESSAGE = 'Another import is running, or you imported a moment ago.';

interface ImportReply {
  status: number;
  body: unknown;
  headers?: Readonly<Record<string, string>>;
}

function endpointReplies(): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}`, capture_count: 0 },
    },
  };
}

function jsonFile() {
  return { name: 'example-export.json', mimeType: 'application/json', buffer: Buffer.from(jsonExportText()) };
}

async function mockImport(page: Page, path: string, reply: ImportReply, requests: string[]): Promise<void> {
  await page.route(
    (url) => url.pathname === path,
    async (route: Route) => {
      const request = route.request();
      requests.push(`${request.method()} ${new URL(request.url()).search}`);
      await route.fulfill({
        status: reply.status,
        contentType: 'application/json',
        headers: reply.headers,
        body: JSON.stringify(reply.body),
      });
    },
  );
}

async function openFromHome(page: Page): Promise<Locator> {
  await page.getByRole('button', { name: DIALOG_NAME, exact: true }).click();
  const dialog = page.getByRole('dialog', { name: DIALOG_NAME });
  await expect(dialog).toBeVisible();
  return dialog;
}

async function chooseFile(dialog: Locator): Promise<void> {
  await dialog.locator('input[type="file"]').setInputFiles(jsonFile());
  await expect(dialog.getByText(EXPORT_TITLE)).toBeVisible();
}

test.describe('import dialog', () => {
  test('a ZIP export is refused in the dialog and never uploaded', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mockApi(page, { 'GET /instance': instanceWith() });
    const requests: string[] = [];
    await mockImport(page, IMPORT_NEW_PATH, { status: 201, body: {} }, requests);
    await page.goto('/');
    const dialog = await openFromHome(page);

    await dialog
      .locator('input[type="file"]')
      .setInputFiles({ name: 'captures.zip', mimeType: 'application/zip', buffer: Buffer.from('PK') });

    await expect(dialog.getByRole('alert')).toHaveText(ZIP_MESSAGE);
    await expect(dialog.getByRole('button', { name: 'Import', exact: true })).toBeDisabled();
    expect(requests).toEqual([]);
  });

  test('a JSON export imports into a new endpoint and opens it', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mockApi(page, endpointReplies());
    const requests: string[] = [];
    await mockImport(
      page,
      IMPORT_NEW_PATH,
      {
        status: 201,
        body: { endpoint_id: ENDPOINT_ID, imported: 2, skipped: 0, reason: null },
        headers: { Location: `/api/v1/endpoints/${ENDPOINT_ID}` },
      },
      requests,
    );
    await page.goto('/');
    const dialog = await openFromHome(page);
    await chooseFile(dialog);

    await dialog.getByRole('button', { name: 'Import', exact: true }).click();

    await expect(dialog.getByText('Imported 2 captures.')).toBeVisible();
    expect(requests).toEqual(['POST ?keep_times=true']);
    await dialog.getByRole('button', { name: 'Open endpoint', exact: true }).click();
    await expect(page).toHaveURL(ENDPOINT_PATH);
  });

  test('the endpoint menu opens the dialog and a busy server shows the wait', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await mockApi(page, endpointReplies());
    await mockCaptureApi(page, ENDPOINT_ID, []);
    const requests: string[] = [];
    await mockImport(
      page,
      IMPORT_INTO_PATH,
      {
        status: 429,
        body: { error: { code: 'rate_limited', message: 'Wait for the running import.' } },
        headers: { 'Retry-After': '1' },
      },
      requests,
    );
    await page.goto(ENDPOINT_PATH);

    await page.getByRole('button', { name: 'Actions', exact: true }).click();
    await page.getByRole('menuitem', { name: DIALOG_NAME, exact: true }).click();
    const dialog = page.getByRole('dialog', { name: DIALOG_NAME });
    await expect(dialog).toBeVisible();
    await chooseFile(dialog);

    await dialog.getByRole('button', { name: 'Import', exact: true }).click();

    await expect(dialog.getByRole('alert')).toHaveText(BUSY_MESSAGE);
    const retry = dialog.getByRole('button', { name: 'Try again', exact: true });
    await expect(retry).toBeDisabled();
    await expect(retry).toBeEnabled();
    expect(requests).toEqual(['POST ?keep_times=true']);
  });

  test('the import dialog fits a 360 px screen without horizontal scrolling', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 740 });
    await mockApi(page, { 'GET /instance': instanceWith() });
    await page.goto('/');
    const dialog = await openFromHome(page);
    await chooseFile(dialog);

    const overflow = await dialog.evaluate((element) => element.scrollWidth - element.clientWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  });

  for (const theme of ['Light', 'Dark'] as const) {
    test(`the import dialog has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
      await page.setViewportSize({ width: 1440, height: 900 });
      await mockApi(page, { 'GET /instance': instanceWith() });
      await page.goto('/');
      await page.getByRole('radio', { name: theme, exact: true }).check();
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
      const dialog = await openFromHome(page);
      await chooseFile(dialog);

      const results = await new AxeBuilder({ page }).analyze();
      expect(results.violations).toEqual([]);
    });
  }
});
