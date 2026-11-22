import { expect, test, type Locator, type Page } from '@playwright/test';
import type { Capture } from '../src/api/types';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { sendResultFixture } from '../src/test/fixtures/composer';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { catalogueFixture, VALID_TEMPLATE_BODY, templatedEndpoint } from '../src/test/fixtures/templating';
import { renderFake } from '../src/test/fixtures/templatingRender';
import { collectViolations, enforceProductionCsp, type CspReport } from './support/csp';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi } from './support/mockApi';
import { SCRIPT_RULE_ID, mockRuleEditorApi } from './support/ruleEditorApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const WIDTHS = [1440, 360] as const;

async function assertClean(page: Page, report: CspReport): Promise<void> {
  const found = await collectViolations(page, report);
  expect(found).toEqual({ violations: [], consoleErrors: [] });
}

async function openEachPopup(page: Page, report: CspReport, scope: Locator): Promise<number> {
  const triggers = scope.locator('button[aria-haspopup]:visible:enabled');
  const total = await triggers.count();
  for (let index = 0; index < total; index += 1) {
    await triggers.nth(index).click();
    const popup = page.getByRole('dialog').or(page.getByRole('menu')).first();
    await expect(popup).toBeVisible();
    await assertClean(page, report);
    await page.keyboard.press('Escape');
    await expect(popup).toBeHidden();
  }
  return total;
}

function captureAt(seq: number): Capture {
  const capture = captureSeries(seq).at(-1);
  if (capture === undefined) {
    throw new Error(`no fixture capture ${seq}`);
  }
  return { ...capture, endpoint_id: ENDPOINT_ID };
}

async function mockEndpointPage(page: Page): Promise<void> {
  await mockApi(page, {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}`, my_role: 'owner' },
    },
    [`GET /endpoints/${ENDPOINT_ID}/rules`]: { body: { items: [], version: 1 } },
  });
  await mockCaptureApi(page, ENDPOINT_ID, captureSeries(3));
  await installFakeStream(page);
}

for (const width of WIDTHS) {
  test.describe(`production CSP at ${width} px`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
    });

    test('home page', async ({ page }) => {
      const report = await enforceProductionCsp(page);
      await mockApi(page, { 'GET /instance': instanceWith() });
      await page.goto('/');
      await expect(page.getByRole('button', { name: 'Create endpoint' })).toBeVisible();
      await assertClean(page, report);
    });

    test('capture detail with its body viewer, its menus and a dialog', async ({ page }) => {
      const report = await enforceProductionCsp(page);
      await mockEndpointPage(page);
      await page.goto(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`);
      const detail = page.getByRole('region', { name: 'Selected capture' });
      await expect(detail.getByRole('textbox', { name: /body/i }).first()).toBeVisible();
      await assertClean(page, report);
      expect(await openEachPopup(page, report, detail)).toBeGreaterThan(0);
      await page.keyboard.press('Shift+?');
      await expect(page.getByRole('dialog')).toBeVisible();
      await assertClean(page, report);
    });

    test('response settings with a template editor', async ({ page }) => {
      const report = await enforceProductionCsp(page);
      const endpoint = {
        ...templatedEndpoint(VALID_TEMPLATE_BODY),
        id: ENDPOINT_ID,
        url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
      };
      await mockApi(page, {
        'GET /instance': instanceWith(),
        [`GET /endpoints/${ENDPOINT_ID}`]: { body: endpoint },
      });
      await mockCaptureApi(page, ENDPOINT_ID, [captureAt(2)]);
      await installFakeStream(page);
      await page.route(`**/api/v1/endpoints/${ENDPOINT_ID}/template/variables*`, (route) =>
        route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(catalogueFixture) }),
      );
      await page.route(`**/api/v1/endpoints/${ENDPOINT_ID}/template/preview`, async (route) => {
        const request = route.request().postDataJSON();
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(renderFake(request, captureAt(2))),
        });
      });
      await page.goto(`${ENDPOINT_PATH}/settings/response`);
      await expect(page.getByRole('textbox', { name: 'Response body (template)', exact: true })).toBeVisible();
      await assertClean(page, report);
      await openEachPopup(page, report, page.locator('main'));
    });

    test('rule editor with a script step', async ({ page }) => {
      const report = await enforceProductionCsp(page);
      await mockRuleEditorApi(page);
      await page.goto(`${ENDPOINT_PATH}/settings/rules/${SCRIPT_RULE_ID}`);
      await expect(page.getByRole('textbox', { name: 'Script', exact: true })).toBeVisible();
      await assertClean(page, report);
      await openEachPopup(page, report, page.locator('main'));
    });

    test('composer with a body editor', async ({ page }) => {
      const report = await enforceProductionCsp(page);
      await mockApi(page, {
        'GET /instance': instanceWith(),
        'GET /send/history': { body: { items: [], next_cursor: null } },
        'POST /send': { body: sendResultFixture },
      });
      await page.goto('/send');
      await expect(page.getByLabel('Target URL')).toBeVisible();
      await assertClean(page, report);
      await openEachPopup(page, report, page.locator('main'));
    });
  });
}
