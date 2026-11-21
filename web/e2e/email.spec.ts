import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import {
  DOCUMENT_PART_INDEX,
  EMAIL_CAPTURE_ID,
  FAKE_IMAGE_BODY,
  FAKE_PNG_PART_INDEX,
  hostileEmailCapture,
  PNG_BODY,
  PNG_PART_INDEX,
  SAFE_FILE_NAME,
  TEXT_PART_BODY,
} from '../src/features/email/testFixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { ENDPOINT_ID, instanceWith, mockApi } from './support/mockApi';

const CAPTURE_PATH = `/e/${ENDPOINT_ID}/c/${EMAIL_CAPTURE_ID}`;
const PARTS_GLOB = `**/api/v1/endpoints/${ENDPOINT_ID}/captures/${EMAIL_CAPTURE_ID}/parts/*`;
const OCTET_STREAM = 'application/octet-stream';

interface OpenedEmail {
  dialogs: string[];
  partRequests: string[];
}

async function openEmail(page: Page, width = 1440): Promise<OpenedEmail> {
  const opened: OpenedEmail = { dialogs: [], partRequests: [] };
  page.on('dialog', (dialog) => {
    opened.dialogs.push(dialog.message());
    void dialog.dismiss();
  });
  await page.setViewportSize({ width, height: 900 });
  await mockApi(page, {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: {
        ...endpointFixture,
        id: ENDPOINT_ID,
        url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
        capture_count: 1,
        last_seq: 1,
      },
    },
  });
  await mockCaptureApi(page, ENDPOINT_ID, [hostileEmailCapture]);
  await page.route(PARTS_GLOB, async (route) => {
    const url = new URL(route.request().url());
    const index = Number(url.pathname.split('/').at(-1));
    opened.partRequests.push(`${index}${url.search}`);
    if (index === 0) {
      await route.fulfill({ status: 200, contentType: 'text/plain; charset=utf-8', body: TEXT_PART_BODY });
    } else if (index === PNG_PART_INDEX) {
      await route.fulfill({ status: 200, contentType: OCTET_STREAM, body: Buffer.from(PNG_BODY) });
    } else if (index === FAKE_PNG_PART_INDEX) {
      await route.fulfill({ status: 200, contentType: OCTET_STREAM, body: Buffer.from(FAKE_IMAGE_BODY) });
    } else {
      await route.fulfill({ status: 200, contentType: OCTET_STREAM, body: 'part' });
    }
  });
  await page.goto(CAPTURE_PATH);
  await expect(page.getByRole('group', { name: 'Email capture' })).toBeVisible();
  return opened;
}

function section(page: Page, name: string): Locator {
  return page.getByRole('group', { name: 'Email capture' }).getByRole('region', { name, exact: true });
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

test('hostile subject, sender and headers render as inert text with visible escapes', async ({ page }) => {
  const opened = await openEmail(page);
  const detail = page.getByRole('group', { name: 'Email capture' });

  await expect(section(page, 'Summary')).toContainText('\\u{202E}drowssap\\u{2069}');
  await expect(section(page, 'Summary')).toContainText('<img src=x onerror=alert(1)>');
  await expect(detail.getByRole('table', { name: 'Message headers' })).toContainText('X-\\u{202E}Name');
  await expect(detail.locator('img, script, iframe, object, embed')).toHaveCount(0);
  expect(opened.dialogs).toEqual([]);
});

test('the plain-text part is shown as escaped text from the text view of its part', async ({ page }) => {
  const opened = await openEmail(page);

  await expect(section(page, 'Text').getByLabel('Plain-text part')).toContainText('Hello \\u{202E}there');
  expect(opened.partRequests).toContain('0?view=text');
});

test('attachments download through the parts route under a sanitized name', async ({ page }) => {
  await openEmail(page);
  const attachments = section(page, 'Attachments');
  const link = attachments.getByRole('link', { name: 'Download ../../etc/\\u{202E}gpj.exe' });

  await expect(link).toHaveAttribute('href', new RegExp(`/parts/${DOCUMENT_PART_INDEX}\\?download=1$`));
  const downloading = page.waitForEvent('download');
  await link.click();

  expect((await downloading).suggestedFilename()).toBe(SAFE_FILE_NAME);
});

test('a raster attachment previews, a disguised one is refused and svg has no preview', async ({ page }) => {
  await openEmail(page);
  const attachments = section(page, 'Attachments');

  await expect(attachments.getByRole('button', { name: 'Preview diagram.svg' })).toHaveCount(0);
  await expect(attachments.getByRole('button', { name: 'Preview huge.png' })).toHaveCount(0);

  await attachments.getByRole('button', { name: 'Preview logo.png' }).click();
  const image = attachments.getByRole('img', { name: /PNG/u });
  await expect(image).toBeVisible();
  await expect.poll(() => image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);

  await attachments.getByRole('button', { name: 'Preview photo.png' }).click();
  await expect(attachments.getByRole('alert')).toContainText('The preview could not be shown');
});

test('the raw source opens on demand as escaped text', async ({ page }) => {
  await openEmail(page);
  const raw = section(page, 'Raw');

  await raw.getByRole('button', { name: 'Show Raw' }).click();

  await expect(raw.getByLabel('Message source')).toContainText('--wl\\u{202E}');
  await expect(raw.getByRole('button', { name: 'Download .eml' })).toBeVisible();
});

test('the narrow layout has no horizontal overflow', async ({ page }) => {
  await openEmail(page, 390);

  await expect(section(page, 'Attachments')).toBeVisible();
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBe(0);
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the email detail has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await openEmail(page);
    await chooseTheme(page, theme);
    await section(page, 'Raw').getByRole('button', { name: 'Show Raw' }).click();
    await section(page, 'Attachments').getByRole('button', { name: 'Preview photo.png' }).click();
    await expect(section(page, 'Raw').getByLabel('Message source')).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
