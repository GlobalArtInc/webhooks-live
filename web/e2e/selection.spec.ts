import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { captureSeries } from '../src/features/endpoint/testFixtures';
import { mockCaptureApi, type CaptureApiState } from './support/captureApi';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const LIST_NAME = 'Captures';
const EXPORT_URL_PATH = `/api/v1/endpoints/${ENDPOINT_ID}/export`;
const ATTACHMENT = 'attachment; filename="webhooks-live-export.json"';

interface ExportFailure {
  status: number;
  code: string;
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

async function openEndpoint(page: Page, captureCount: number): Promise<CaptureApiState> {
  await mockApi(page, endpointReplies(captureCount));
  const state = await mockCaptureApi(page, ENDPOINT_ID, captureSeries(captureCount));
  await page.goto(ENDPOINT_PATH);
  await expect(page.getByRole('listbox', { name: LIST_NAME })).toBeVisible();
  return state;
}

async function mockExport(page: Page, failure: ExportFailure | null, requests: string[]): Promise<void> {
  await page.route(
    (url) => url.pathname === EXPORT_URL_PATH,
    async (route) => {
      const request = route.request();
      requests.push(`${request.method()} ${new URL(request.url()).search}`);
      if (failure !== null) {
        await route.fulfill({
          status: failure.status,
          contentType: 'application/json',
          headers: failure.status === 429 ? { 'Retry-After': '1' } : undefined,
          body: JSON.stringify({ error: { code: failure.code, message: 'Export refused.' } }),
        });
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        headers: { 'Content-Disposition': ATTACHMENT },
        body: '[]',
      });
    },
  );
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

function selectionBar(page: Page) {
  return page.getByRole('group', { name: 'Selection' });
}

test.describe('select mode', () => {
  test('select all visible and delete with undo keeps every capture until the undo window closes', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    const state = await openEndpoint(page, 5);
    const listbox = page.getByRole('listbox', { name: LIST_NAME });

    await page.getByRole('button', { name: 'Select', exact: true }).click();
    await expect(listbox).toHaveAttribute('aria-multiselectable', 'true');
    await listbox.getByRole('option').first().click();
    await expect(selectionBar(page).getByRole('status')).toHaveText('1 selected');

    await page.getByRole('button', { name: 'Select all visible' }).click();
    await expect(selectionBar(page).getByRole('status')).toHaveText('5 selected');

    await page.getByRole('button', { name: 'Delete selected' }).click();
    const dialog = page.getByRole('dialog', { name: 'Delete 5 captures?' });
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Delete', exact: true }).click();
    await expect(page.getByRole('status').filter({ hasText: '5 captures deleted' })).toBeVisible();

    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(listbox.getByRole('option')).toHaveCount(5);
    expect(state.deleted).toEqual([]);
  });

  test('dismissing the undo toast deletes the checked captures on the server', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    const state = await openEndpoint(page, 4);
    const listbox = page.getByRole('listbox', { name: LIST_NAME });

    await page.getByRole('button', { name: 'Select', exact: true }).click();
    await listbox.getByRole('option').nth(0).click();
    await listbox.getByRole('option').nth(2).click();
    await expect(selectionBar(page).getByRole('status')).toHaveText('2 selected');

    await page.getByRole('button', { name: 'Delete selected' }).click();
    await page
      .getByRole('dialog', { name: 'Delete 2 captures?' })
      .getByRole('button', { name: 'Delete', exact: true })
      .click();
    await page.getByRole('button', { name: 'Dismiss' }).click();

    await expect.poll(() => state.deleted.length).toBe(2);
  });

  test('space toggles the focused capture and enters select mode', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openEndpoint(page, 3);
    const listbox = page.getByRole('listbox', { name: LIST_NAME });

    await listbox.getByRole('option').nth(1).click();
    await listbox.focus();
    await page.keyboard.press('Space');

    await expect(listbox).toHaveAttribute('aria-multiselectable', 'true');
    await expect(listbox.getByRole('option').nth(1)).toHaveAttribute('aria-selected', 'true');
    await expect(selectionBar(page).getByRole('status')).toHaveText('1 selected');
  });

  test('a long press on a narrow row starts select mode with that row checked', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 800 });
    await openEndpoint(page, 3);
    const listbox = page.getByRole('listbox', { name: LIST_NAME });
    const box = await listbox.getByRole('option').first().boundingBox();
    if (box === null) {
      throw new Error('The first capture row is not visible');
    }

    await page.mouse.move(box.x + box.width / 4, box.y + box.height / 2);
    await page.mouse.down();
    await expect(listbox).toHaveAttribute('aria-multiselectable', 'true');
    await page.mouse.up();

    await expect(selectionBar(page).getByRole('status')).toHaveText('1 selected');
    await expect(listbox.getByRole('option').first()).toHaveAttribute('aria-selected', 'true');
  });
});

test.describe('export dialog', () => {
  test('exporting the checked captures downloads the file the server named', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openEndpoint(page, 3);
    const requests: string[] = [];
    await mockExport(page, null, requests);
    const listbox = page.getByRole('listbox', { name: LIST_NAME });

    await page.getByRole('button', { name: 'Select', exact: true }).click();
    await listbox.getByRole('option').nth(0).click();
    await listbox.getByRole('option').nth(1).click();
    await selectionBar(page).getByRole('button', { name: 'Export', exact: true }).click();

    const dialog = page.getByRole('dialog', { name: 'Export captures' });
    await expect(dialog.getByRole('radio', { name: 'Selected' })).toBeChecked();
    const download = page.waitForEvent('download');
    await dialog.getByRole('button', { name: 'Export', exact: true }).click();
    expect((await download).suggestedFilename()).toBe('webhooks-live-export.json');
    await expect(dialog.getByRole('status')).toContainText('Ready: JSON');
    await expect.poll(() => requests).toEqual(['POST ']);
  });

  test('exporting all captures without a selection streams a GET with the chosen format', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openEndpoint(page, 3);
    const requests: string[] = [];
    await mockExport(page, null, requests);

    await page.getByRole('button', { name: 'Export', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Export captures' });
    await expect(dialog.getByRole('radio', { name: 'All captures' })).toBeChecked();
    await dialog.getByRole('radio', { name: 'CSV' }).check();
    const download = page.waitForEvent('download');
    await dialog.getByRole('button', { name: 'Export', exact: true }).click();

    expect((await download).suggestedFilename()).toBe('webhooks-live-export.json');
    await expect.poll(() => requests).toEqual(['GET ?format=csv']);
  });

  test('a busy endpoint shows the retry time inside the dialog', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openEndpoint(page, 2);
    await mockExport(page, { status: 429, code: 'rate_limited' }, []);
    const listbox = page.getByRole('listbox', { name: LIST_NAME });

    await page.getByRole('button', { name: 'Select', exact: true }).click();
    await listbox.getByRole('option').nth(0).click();
    await selectionBar(page).getByRole('button', { name: 'Export', exact: true }).click();
    await page
      .getByRole('dialog', { name: 'Export captures' })
      .getByRole('button', { name: 'Export', exact: true })
      .click();

    await expect(page.getByRole('dialog').getByRole('alert')).toHaveText(
      'Another export is running for this endpoint. Try again in 1 s.',
    );
  });

  test('a gone endpoint shows its own message inside the dialog', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openEndpoint(page, 2);
    await mockExport(page, { status: 410, code: 'endpoint_gone' }, []);
    const listbox = page.getByRole('listbox', { name: LIST_NAME });

    await page.getByRole('button', { name: 'Select', exact: true }).click();
    await listbox.getByRole('option').nth(0).click();
    await selectionBar(page).getByRole('button', { name: 'Export', exact: true }).click();
    await page
      .getByRole('dialog', { name: 'Export captures' })
      .getByRole('button', { name: 'Export', exact: true })
      .click();

    await expect(page.getByRole('dialog').getByRole('alert')).toHaveText(
      'This endpoint no longer exists or you no longer have access to it.',
    );
  });

  for (const theme of ['Light', 'Dark'] as const) {
    test(`the export dialog has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
      await page.setViewportSize({ width: 1440, height: 900 });
      await openEndpoint(page, 3);
      await chooseTheme(page, theme);
      await page.getByRole('button', { name: 'Export', exact: true }).click();
      await expect(page.getByRole('dialog', { name: 'Export captures' })).toBeVisible();

      const results = await new AxeBuilder({ page }).analyze();
      expect(results.violations).toEqual([]);
    });
  }
});
