import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import type { Capture, Endpoint } from '../src/api/types';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { catalogueFixture, INVALID_TEMPLATE_BODY, templatedEndpoint } from '../src/test/fixtures/templating';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi } from './support/mockApi';

interface Size {
  width: number;
  height: number;
}

interface Edges {
  top: number;
  bottom: number;
}

const WIDE: Size = { width: 1440, height: 900 };
const MEDIUM: Size = { width: 1024, height: 768 };
const MEDIUM_LOW: Size = { width: 900, height: 768 };
const MEDIUM_FLOOR: Size = { width: 768, height: 768 };
const NARROW: Size = { width: 360, height: 740 };
const LONG_PATH = '/webhooks/stripe/payment-intents/succeeded/evt_1Q9';
const MIN_LIST_WIDTH = 358;
const MIN_PATH_CHARS = 20;
const MIN_URL_HEAD_CHARS = 24;
const EDGE_TOLERANCE = 1;

function longPathSeries(count: number): Capture[] {
  return captureSeries(count).map((capture) => ({ ...capture, path: `${LONG_PATH}/${capture.seq}` }));
}

async function openPage(page: Page, size: Size, captures: Capture[], path: string, body?: string): Promise<void> {
  const endpoint: Endpoint = {
    ...(body === undefined ? endpointFixture : templatedEndpoint(body)),
    id: ENDPOINT_ID,
    url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
    capture_count: captures.length,
    last_seq: captures.length,
  };
  await page.setViewportSize(size);
  await installFakeStream(page);
  await mockApi(page, {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: endpoint },
    'GET /folders': { body: { items: [] } },
    [`GET /endpoints/${ENDPOINT_ID}/aliases`]: { body: { items: [] } },
    [`GET /endpoints/${ENDPOINT_ID}/template/variables`]: { body: catalogueFixture },
  });
  await mockCaptureApi(page, ENDPOINT_ID, captures);
  await page.goto(path);
}

async function openCapture(page: Page, size: Size): Promise<void> {
  await openPage(page, size, longPathSeries(30), `/e/${ENDPOINT_ID}/c/${captureIdFor(5)}`);
  await expect(page.getByRole('listbox', { name: 'Captures' })).toBeVisible();
  await expect(page.getByRole('heading', { name: /^Capture / })).toBeVisible();
}

async function glyphCount(node: Locator): Promise<number> {
  return node.evaluate((element) => {
    const probe = document.createElement('span');
    probe.style.font = getComputedStyle(element).font;
    probe.style.position = 'absolute';
    probe.textContent = 'x'.repeat(20);
    document.body.append(probe);
    const glyph = probe.getBoundingClientRect().width / 20;
    probe.remove();
    return element.clientWidth / glyph;
  });
}

async function scrollEdges(bar: Locator): Promise<Edges> {
  return bar.evaluate((element) => {
    let node: HTMLElement | null = element.parentElement;
    while (node !== null && !['auto', 'scroll'].includes(getComputedStyle(node).overflowY)) {
      node = node.parentElement;
    }
    const box = (node ?? document.documentElement).getBoundingClientRect();
    return { top: box.top, bottom: node === null ? window.innerHeight : box.bottom };
  });
}

async function scrollToEnd(bar: Locator): Promise<void> {
  await bar.evaluate((element) => {
    let node: HTMLElement | null = element.parentElement;
    while (node !== null && !['auto', 'scroll'].includes(getComputedStyle(node).overflowY)) {
      node = node.parentElement;
    }
    if (node === null) {
      window.scrollTo(0, document.documentElement.scrollHeight);
    } else {
      node.scrollTop = node.scrollHeight;
    }
  });
}

for (const size of [WIDE, MEDIUM, MEDIUM_LOW, MEDIUM_FLOOR]) {
  test(`at ${size.width} px the list keeps 20 path characters and the header keeps a readable capture URL`, async ({
    page,
  }) => {
    await openCapture(page, size);

    const list = page.getByRole('search').locator('xpath=..');
    expect((await list.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(MIN_LIST_WIDTH);
    const row = page.getByRole('listbox', { name: 'Captures' }).getByRole('option').first();
    expect(await glyphCount(row.locator(`[title^="${LONG_PATH}"]`))).toBeGreaterThanOrEqual(MIN_PATH_CHARS);

    const url = page.locator(`code [title$="${ENDPOINT_ID}"]`);
    const head = url.locator('span').nth(0);
    expect(await glyphCount(head)).toBeGreaterThanOrEqual(MIN_URL_HEAD_CHARS);
    const urlBox = await url.boundingBox();
    const tail = await url.locator('span').last().boundingBox();
    expect((tail?.x ?? 0) + (tail?.width ?? 0)).toBeLessThanOrEqual((urlBox?.x ?? 0) + (urlBox?.width ?? 0) + 1);
    const copy = await page.getByRole('button', { name: 'Copy URL', exact: true }).first().boundingBox();
    expect((copy?.x ?? 0) + (copy?.width ?? 0)).toBeLessThanOrEqual(size.width);

    const overflow = await page.evaluate(() => ({
      document: document.documentElement.scrollWidth - document.documentElement.clientWidth,
      main: (document.querySelector('main')?.scrollWidth ?? 0) - (document.querySelector('main')?.clientWidth ?? 0),
    }));
    expect(overflow).toEqual({ document: 0, main: 0 });
  });
}

test('at 1024 the endpoint list has no axe violations', async ({ page }) => {
  await openCapture(page, MEDIUM);

  const result = await new AxeBuilder({ page }).analyze();
  expect(result.violations).toEqual([]);
});

for (const size of [WIDE, MEDIUM, NARROW]) {
  test(`at ${size.width} px the save bar is docked to the bottom of the workspace and hides no row`, async ({
    page,
  }) => {
    await openPage(page, size, [], `/e/${ENDPOINT_ID}/settings/response`, INVALID_TEMPLATE_BODY);
    await expect(page.getByRole('heading', { level: 2, name: 'Response' })).toBeVisible();
    const bar = page.getByRole('button', { name: 'Save', exact: true }).locator('xpath=../..');
    await expect(bar).toBeVisible();

    const docked = async (): Promise<number> => {
      const edges = await scrollEdges(bar);
      const box = await bar.boundingBox();
      return Math.abs((box?.y ?? 0) + (box?.height ?? 0) - edges.bottom);
    };
    expect(await docked()).toBeLessThanOrEqual(EDGE_TOLERANCE);

    await expect(page.getByRole('textbox', { name: 'Response body (template)', exact: true })).toBeVisible();

    const aboveOf = (): Promise<number> =>
      bar.evaluate((element) => element.previousElementSibling?.getBoundingClientRect().bottom ?? 0);
    await expect
      .poll(async () => {
        await scrollToEnd(bar);
        const barBox = await bar.boundingBox();
        return (barBox?.y ?? 0) + EDGE_TOLERANCE - (await aboveOf());
      })
      .toBeGreaterThanOrEqual(0);
    expect(await docked()).toBeLessThanOrEqual(EDGE_TOLERANCE);
    expect(await aboveOf()).toBeGreaterThan(0);
  });
}

test('at 360 a dialog opened from the Actions menu gives the focus back to Actions on Escape', async ({ page }) => {
  await openPage(page, NARROW, longPathSeries(3), `/e/${ENDPOINT_ID}`);
  await expect(page.getByRole('listbox', { name: 'Captures' })).toBeVisible();
  const actions = page.getByRole('button', { name: 'Actions', exact: true });

  await actions.click();
  await page.getByRole('menuitem', { name: 'Delete all' }).click();
  const dialog = page.getByRole('dialog', { name: 'Delete all captures?' });
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute('aria-modal', 'true');
  await expect(page.locator('#root')).toHaveAttribute('inert', '');
  const result = await new AxeBuilder({ page }).analyze();
  expect(result.violations).toEqual([]);

  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(actions).toBeFocused();
  await expect(page.locator('#root')).not.toHaveAttribute('inert', /.*/);
});

test('at 360 the menu drawer is modal and the app behind it is inert', async ({ page }) => {
  await openPage(page, NARROW, longPathSeries(3), `/e/${ENDPOINT_ID}`);
  await expect(page.getByRole('listbox', { name: 'Captures' })).toBeVisible();
  const menu = page.getByRole('button', { name: 'Menu' });

  await menu.click();
  const drawer = page.getByRole('dialog', { name: 'Menu' });
  await expect(drawer).toHaveAttribute('aria-modal', 'true');
  await expect(page.locator('#root')).toHaveAttribute('inert', '');
  expect(await page.evaluate(() => document.getElementById('root')?.contains(document.activeElement))).toBe(false);

  await page.keyboard.press('Escape');
  await expect(drawer).toBeHidden();
  await expect(page.locator('#root')).not.toHaveAttribute('inert', /.*/);
  await expect(menu).toBeFocused();
});
