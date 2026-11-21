import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page, type Route } from '@playwright/test';
import type { Capture, Endpoint } from '../src/api/types';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import type { PreviewRequest } from '../src/features/templating/types';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import {
  catalogueFixture,
  INVALID_TEMPLATE_BODY,
  templatedEndpoint,
  VALID_TEMPLATE_BODY,
} from '../src/test/fixtures/templating';
import { renderFake } from '../src/test/fixtures/templatingRender';
import { mockCaptureApi, type CaptureApiState } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi } from './support/mockApi';

const RESPONSE_PATH = `/e/${ENDPOINT_ID}/settings/response`;
const BODY_LABEL = 'Response body (template)';
const SAVE_BLOCKED = 'Fix the template errors to save.';
const POLL_WAIT_MS = 15_000;

interface Rig {
  state: CaptureApiState;
  previews: PreviewRequest[];
  patches: unknown[];
}

function endpointOf(body: string): Endpoint {
  return { ...templatedEndpoint(body), id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}` };
}

function captureAt(seq: number): Capture {
  const capture = captureSeries(seq).at(-1);
  if (capture === undefined) {
    throw new Error(`no fixture capture ${seq}`);
  }
  return { ...capture, endpoint_id: ENDPOINT_ID };
}

function fulfillJson(route: Route, body: unknown, status = 200): Promise<void> {
  return route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
}

async function openResponse(page: Page, body: string, captures: Capture[] = captureSeries(2)): Promise<Rig> {
  const endpoint = endpointOf(body);
  await mockApi(page, {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: endpoint },
  });
  const state = await mockCaptureApi(page, ENDPOINT_ID, captures);
  await installFakeStream(page);
  const rig: Rig = { state, previews: [], patches: [] };
  await page.route(`**/api/v1/endpoints/${ENDPOINT_ID}/template/variables*`, (route) =>
    fulfillJson(route, catalogueFixture),
  );
  await page.route(`**/api/v1/endpoints/${ENDPOINT_ID}/template/preview`, async (route) => {
    const request: PreviewRequest = route.request().postDataJSON();
    rig.previews.push(request);
    const capture = rig.state.captures.find((item) => item.id === request.capture_id) ?? null;
    await fulfillJson(route, renderFake(request, capture));
  });
  await page.route(`**/api/v1/endpoints/${ENDPOINT_ID}`, async (route) => {
    if (route.request().method() !== 'PATCH') {
      await route.fallback();
      return;
    }
    rig.patches.push(route.request().postDataJSON());
    await fulfillJson(route, endpoint);
  });
  await page.goto(RESPONSE_PATH);
  await expect(page.getByRole('heading', { level: 2, name: 'Response' })).toBeVisible();
  return rig;
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

test.describe('response templating', () => {
  test('an error in the body underlines, lists and blocks Save until it is fixed', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openResponse(page, INVALID_TEMPLATE_BODY);
    const body = page.getByRole('textbox', { name: BODY_LABEL, exact: true });
    const problems = page.getByRole('group', { name: 'Template problems' });

    await expect(problems.getByText('Errors 1', { exact: true })).toBeVisible();
    await expect(problems.getByText('Unknown filter "isodate"')).toBeVisible();
    await expect(page.locator('.cm-lintRange-error').first()).toBeVisible();
    await expect(page.getByText('Nothing to show yet. Fix the errors in the editor.')).toBeVisible();

    await page.getByLabel('Status code').fill('201');
    await expect(page.getByRole('button', { name: 'Save' })).toBeDisabled();
    await expect(page.getByText(SAVE_BLOCKED)).toBeVisible();

    await problems.getByRole('button', { name: /^Go to Body/ }).click();
    await expect(body).toBeFocused();

    await body.fill(VALID_TEMPLATE_BODY);
    await expect(problems).toHaveCount(0);
    await expect(page.locator('.cm-lintRange-error')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Save' })).toBeEnabled();
    await expect(page.locator('pre')).toContainText('"path": "/hook/2"');
  });

  test('the preview follows the latest capture, and Pin fixes one in the URL', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    const rig = await openResponse(page, VALID_TEMPLATE_BODY);
    await expect(page.locator('pre')).toContainText('"path": "/hook/2"');

    rig.state.captures.push(captureAt(3));
    await expect(page.locator('pre')).toContainText('"path": "/hook/3"', { timeout: POLL_WAIT_MS });

    await page.getByRole('button', { name: 'Pin' }).click();
    await expect(page).toHaveURL(new RegExp(`preview=${captureIdFor(3)}`));
    rig.state.captures.push(captureAt(4));
    await page.reload();
    await expect(page.getByText('Pinned to one capture')).toBeVisible();
    await expect(page.locator('pre')).toContainText('"path": "/hook/3"');

    await page.getByRole('button', { name: 'Follow latest' }).click();
    await expect(page).not.toHaveURL(/preview=/);
    await expect(page.locator('pre')).toContainText('"path": "/hook/4"', { timeout: POLL_WAIT_MS });
  });

  test('the variable picker inserts a catalogue entry at the cursor of the body', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openResponse(page, '{}');
    const body = page.getByRole('textbox', { name: BODY_LABEL, exact: true });

    await page.getByRole('button', { name: 'Insert variable' }).click();
    await page.getByRole('combobox', { name: 'Filter variables' }).fill('json.id');
    await page.keyboard.press('Enter');

    await expect(body).toBeFocused();
    await expect(body).toContainText('{{ request.json.id }}');
  });

  test('saving sends the Use variables flag with the response', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    const rig = await openResponse(page, VALID_TEMPLATE_BODY);

    await page.getByLabel('Delay (ms)').fill('250');
    await page.getByRole('button', { name: 'Save' }).click();

    await expect.poll(() => rig.patches.length).toBe(1);
    expect(rig.patches[0]).toMatchObject({ response: { templated: true, delay_ms: 250 } });
  });

  test('narrow screens switch between Edit, Preview and Errors, and Go returns to the editor', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 740 });
    await openResponse(page, INVALID_TEMPLATE_BODY);
    const tabs = page.getByRole('tablist', { name: 'Response editor view' });

    await expect(tabs.getByRole('tab', { name: 'Errors 1' })).toBeVisible();
    await tabs.getByRole('tab', { name: 'Preview' }).click();
    await expect(page.getByText('Nothing to show yet. Fix the errors in the editor.')).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);

    await tabs.getByRole('tab', { name: 'Errors 1' }).click();
    await page.getByRole('button', { name: /^Go to Body/ }).click();
    await expect(tabs.getByRole('tab', { name: 'Edit' })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('textbox', { name: BODY_LABEL, exact: true })).toBeFocused();
    expect(await horizontalOverflow(page)).toBe(0);
  });

  test('on medium screens the preview panel hides and stays hidden after a reload', async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 768 });
    await openResponse(page, VALID_TEMPLATE_BODY);
    await expect(page.locator('pre')).toContainText('"path": "/hook/2"');

    await page.getByRole('button', { name: 'Hide' }).click();
    await expect(page.locator('pre')).toBeHidden();
    await page.reload();
    await expect(page.getByRole('button', { name: 'Show preview' })).toBeVisible();
    await expect(page.locator('pre')).toBeHidden();
    expect(await horizontalOverflow(page)).toBe(0);
  });

  for (const theme of ['Light', 'Dark'] as const) {
    test(`the editor with a render, and with an error, has no axe violations in the ${theme.toLowerCase()} theme`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1440, height: 900 });
      await openResponse(page, VALID_TEMPLATE_BODY);
      await chooseTheme(page, theme);
      await expect(page.locator('pre')).toContainText('"path": "/hook/2"');
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);

      await page.getByRole('textbox', { name: BODY_LABEL, exact: true }).fill(INVALID_TEMPLATE_BODY);
      await expect(page.getByRole('group', { name: 'Template problems' })).toBeVisible();
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    });
  }
});
