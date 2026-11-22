import { expect, test, type Locator, type Page } from '@playwright/test';
import type { Capture } from '../src/api/types';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints } from '../src/features/endpoints/storage';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { disabledButtonContrasts } from './support/contrast';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, apiError, instanceWith, mockApi } from './support/mockApi';
import { mockRuleEditorApi } from './support/ruleEditorApi';

interface StorageSeed {
  key: string;
  value: string;
}

interface Size {
  width: number;
  height: number;
}

const WIDE: Size = { width: 1440, height: 900 };
const MEDIUM: Size = { width: 768, height: 900 };
const NARROW: Size = { width: 360, height: 740 };
const NARROWEST: Size = { width: 320, height: 640 };
const UNBROKEN_TITLE = 'X'.repeat(100);
const SPACED_TITLE = 'quarterly payment webhook for the staging environment '.repeat(2).slice(0, 100);
const MIN_CONTRAST = 3;
const LINE_CLAMP = 4;
const LONG_URL_LENGTH = 2000;
const TOLERANCE = 1;

async function openEndpoint(page: Page, size: Size, title: string | null, captures: Capture[], path: string) {
  await page.setViewportSize(size);
  await installFakeStream(page);
  const seed: StorageSeed = {
    key: ENDPOINTS_STORAGE_KEY,
    value: serializeEndpoints([
      {
        id: ENDPOINT_ID,
        label: null,
        ...(title === null ? {} : { title }),
        added: 1,
        lastOpened: 2,
        lastSeenCaptureId: null,
      },
    ]),
  };
  await page.addInitScript((entry: StorageSeed) => {
    window.localStorage.setItem(entry.key, entry.value);
  }, seed);
  await mockApi(page, {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: {
        ...endpointFixture,
        id: ENDPOINT_ID,
        title,
        url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
        capture_count: captures.length,
        last_seq: captures.length,
      },
    },
    'GET /folders': { body: { items: [] } },
    [`GET /endpoints/${ENDPOINT_ID}/aliases`]: { body: { items: [] } },
  });
  await mockCaptureApi(page, ENDPOINT_ID, captures);
  await page.goto(path);
}

async function pageOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

async function overflow(node: Locator): Promise<number> {
  return node.evaluate((element) => element.scrollWidth - element.clientWidth);
}

for (const [kind, title] of [
  ['unbroken', UNBROKEN_TITLE],
  ['spaced', SPACED_TITLE],
] as const) {
  for (const size of [WIDE, NARROW]) {
    test(`a 100-character ${kind} title is cut with an ellipsis in the rail, the list and the home page at ${size.width} px`, async ({
      page,
    }) => {
      await openEndpoint(page, size, title, captureSeries(3), `/e/${ENDPOINT_ID}`);
      let container: Locator;
      if (size.width < 768) {
        await page.getByRole('button', { name: 'Menu', exact: true }).click();
        container = page.getByRole('dialog', { name: 'Menu' });
      } else {
        container = page.locator('aside');
      }
      const link = container.getByRole('link', { name: title });
      await expect(link).toBeVisible();
      await expect(link).toHaveAttribute('title', title);
      expect(await overflow(container)).toBeLessThanOrEqual(TOLERANCE);
      const text = link.locator('span');
      expect(await overflow(text)).toBeGreaterThan(0);
      expect(await text.evaluate((element) => getComputedStyle(element).textOverflow)).toBe('ellipsis');

      await page.goto('/');
      const name = page.getByRole('main').getByText(title, { exact: true });
      await expect(name).toBeVisible();
      await expect(name).toHaveAttribute('title', title);
      expect(await pageOverflow(page)).toBeLessThanOrEqual(TOLERANCE);
      expect(await name.evaluate((element) => getComputedStyle(element).textOverflow)).toBe('ellipsis');
      if (size.width < 768) {
        expect(await overflow(name)).toBeGreaterThan(0);
      }

      await page.goto('/endpoints');
      const card = page.getByRole('main').getByRole('link', { name: title });
      await expect(card).toBeVisible();
      expect(await pageOverflow(page)).toBeLessThanOrEqual(TOLERANCE);
    });
  }
}

for (const theme of ['light', 'dark'] as const) {
  test(`every disabled button keeps 3:1 text contrast in the ${theme} theme`, async ({ page }) => {
    await page.setViewportSize(WIDE);
    await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
    await mockRuleEditorApi(page);
    await page.goto(`/e/${ENDPOINT_ID}/settings/rules/new`);
    await expect(page.getByRole('textbox', { name: 'Rule name' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Run', exact: true })).toBeDisabled();

    const editor = await disabledButtonContrasts(page);
    expect(editor.map((entry) => entry.name)).toContain('Run');

    await page.goto(`/e/${ENDPOINT_ID}`);
    await expect(page.getByRole('button', { name: 'First', exact: true })).toBeDisabled();
    const list = await disabledButtonContrasts(page);
    expect(list.map((entry) => entry.name)).toContain('First');

    for (const entry of [...editor, ...list]) {
      expect(entry.ratio, `${entry.name} in the ${theme} theme`).toBeGreaterThanOrEqual(MIN_CONTRAST);
    }
  });
}

test('the settings tab strip shows a cue for hidden tabs and keeps the selected tab in view', async ({ page }) => {
  await openEndpoint(page, MEDIUM, null, captureSeries(3), `/e/${ENDPOINT_ID}/settings/general`);
  const nav = page.getByRole('navigation', { name: 'Endpoint configuration' });
  const list = nav.locator('ul');
  await expect(nav.getByRole('link').first()).toBeVisible();
  expect(await overflow(list)).toBeGreaterThan(0);
  await expect(nav).toHaveAttribute('data-fade-end', 'true');
  await expect(nav).toHaveAttribute('data-fade-start', 'false');
  const first = await nav.getByRole('link').first().boundingBox();
  const frame = await nav.boundingBox();
  expect(first?.x ?? -1).toBeGreaterThanOrEqual(frame?.x ?? 0);

  await list.evaluate((element) => {
    element.scrollLeft = element.scrollWidth;
  });
  await expect(nav).toHaveAttribute('data-fade-end', 'false');
  await expect(nav).toHaveAttribute('data-fade-start', 'true');

  await list.evaluate((element) => {
    element.scrollLeft = 0;
  });
  const last = nav.getByRole('link').last();
  await last.evaluate((element) => {
    element.scrollIntoView({ inline: 'nearest' });
  });
  await last.click();
  await expect(last).toHaveAttribute('aria-current', 'page');
  const lastBox = await last.boundingBox();
  const listBox = await list.boundingBox();
  expect((lastBox?.x ?? 0) + (lastBox?.width ?? 0)).toBeLessThanOrEqual((listBox?.x ?? 0) + (listBox?.width ?? 0) + 1);
});

test('the capture detail tab strip starts at the left edge and brings the chosen section into view', async ({
  page,
}) => {
  const [base] = captureSeries(1);
  const formCapture: Capture = { ...(base as Capture), form: { fields: [{ name: 'a', value: '1' }], files: [] } };
  await openEndpoint(page, NARROWEST, null, [formCapture], `/e/${ENDPOINT_ID}/c/${captureIdFor(1)}`);
  const nav = page.getByRole('navigation', { name: 'Capture sections' });
  const list = nav.locator('ul');
  await expect(nav.getByRole('button').first()).toBeVisible();
  expect(await list.evaluate((element) => element.scrollLeft)).toBe(0);
  const first = await nav.getByRole('button').first().boundingBox();
  const frame = await nav.boundingBox();
  expect(first?.x ?? -1).toBeGreaterThanOrEqual(frame?.x ?? 0);
  await expect(nav).toHaveAttribute('data-fade-end', 'true');

  await nav.getByRole('button').last().click();
  await expect
    .poll(async () => {
      const box = await nav.getByRole('button').last().boundingBox();
      const view = await list.boundingBox();
      return (box?.x ?? 0) + (box?.width ?? 0) <= (view?.x ?? 0) + (view?.width ?? 0) + 1;
    })
    .toBe(true);
});

function longUrlCapture(url: string): Capture {
  const [base] = captureSeries(1);
  return { ...(base as Capture), url, path: '/long', query_string: '' };
}

function segmentedUrl(): string {
  let url = `${PUBLIC_URL}/${ENDPOINT_ID}/orders?`;
  for (let index = 0; url.length < LONG_URL_LENGTH; index += 1) {
    url += `customer_${index}=value_${index}&`;
  }
  return url.slice(0, LONG_URL_LENGTH);
}

for (const size of [{ width: 1024, height: 800 }, NARROW]) {
  test(`a 2000-character URL is clamped to four lines, expands on demand and still copies whole at ${size.width} px`, async ({
    page,
    context,
  }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    const url = segmentedUrl();
    await openEndpoint(page, size, null, [longUrlCapture(url)], `/e/${ENDPOINT_ID}/c/${captureIdFor(1)}`);
    const summary = page.getByRole('heading', { name: 'Summary', exact: true });
    await expect(summary).toBeVisible();
    const code = page.locator('code[data-expanded]');
    const toggle = page.getByRole('button', { name: 'Show full URL' });
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');

    const lineHeight = await code.evaluate((element) => parseFloat(getComputedStyle(element).lineHeight));
    const clamped = (await code.boundingBox())?.height ?? 0;
    expect(clamped).toBeLessThanOrEqual(lineHeight * LINE_CLAMP + TOLERANCE);

    await toggle.click();
    const less = page.getByRole('button', { name: 'Show less' });
    await expect(less).toHaveAttribute('aria-expanded', 'true');
    expect((await code.boundingBox())?.height ?? 0).toBeGreaterThan(lineHeight * LINE_CLAMP * 2);
    expect(await pageOverflow(page)).toBeLessThanOrEqual(TOLERANCE);

    const segments = await code.evaluate((element) => {
      const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
      let split = 0;
      for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
        const range = document.createRange();
        range.selectNodeContents(node);
        const lines = new Set([...range.getClientRects()].map((rect) => Math.round(rect.top)));
        if (lines.size > 1) {
          split += 1;
        }
      }
      return split;
    });
    expect(segments).toBe(0);

    await less.click();
    await expect(page.getByRole('button', { name: 'Show full URL' })).toBeVisible();

    await page.getByRole('button', { name: 'Copy URL' }).last().click();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(url);
  });
}

test('a short capture URL shows no toggle', async ({ page }) => {
  await openEndpoint(page, WIDE, null, captureSeries(2), `/e/${ENDPOINT_ID}/c/${captureIdFor(1)}`);
  await expect(page.getByRole('heading', { name: 'Summary', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Show full URL' })).toHaveCount(0);
});

test('the shortcuts dialog puts the focus on its title when it opens', async ({ page }) => {
  await openEndpoint(page, NARROW, null, captureSeries(2), `/e/${ENDPOINT_ID}`);
  await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await page.getByRole('dialog', { name: 'Menu' }).getByRole('button', { name: 'Shortcuts', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Keyboard shortcuts' })).toBeFocused();
});

test('an expired endpoint renders its state without logging the handled error', async ({ page }) => {
  const logged: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') {
      logged.push(message.text());
    }
  });
  await page.setViewportSize(WIDE);
  await installFakeStream(page);
  await mockApi(page, {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: apiError(410, 'endpoint_gone', 'This endpoint has expired.'),
  });
  await page.goto(`/e/${ENDPOINT_ID}`);
  await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible();
  await expect(page.getByText(/expired|gone/i).first()).toBeVisible();
  expect(logged.filter((text) => text.includes('ApiError'))).toEqual([]);
});
