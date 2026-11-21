import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import type { Capture } from '../src/api/types';
import type { RuleRun } from '../src/api/ruleRunTypes';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import { captureRunsFixture, RERUN_FIXTURE_ID, ruleRunFixture } from '../src/features/rules/runlogs/fixtures';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream, openLatestStream, pushStreamEvent, streamCount } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const TARGET_SEQ = 3;
const CAPTURE_COUNT = 5;
const CAPTURE_PATH = `/e/${ENDPOINT_ID}/c/${captureIdFor(TARGET_SEQ)}`;
const PREFS_KEY = 'webhooks-live:prefs:v1';
const MOBILE = { width: 360, height: 800 };
const DESKTOP = { width: 1440, height: 900 };
const RERUN_PATH = `/api/v1/endpoints/${ENDPOINT_ID}/captures/${captureIdFor(TARGET_SEQ)}/rule-runs`;

const THEMES = ['light', 'dark'] as const;

interface StoredPrefs {
  key: string;
  value: string;
}

function runsOfTarget(): RuleRun[] {
  return captureRunsFixture.map((run) => ({ ...run, capture_id: captureIdFor(TARGET_SEQ) }));
}

function captureSet(): Capture[] {
  return captureSeries(CAPTURE_COUNT).map((capture) =>
    capture.seq === TARGET_SEQ ? { ...capture, rule_runs: runsOfTarget() } : capture,
  );
}

function endpointReplies(): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: {
        ...endpointFixture,
        id: ENDPOINT_ID,
        url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
        capture_count: CAPTURE_COUNT,
        last_seq: CAPTURE_COUNT,
      },
    },
  };
}

async function openCapture(page: Page, theme: 'light' | 'dark', viewport: { width: number; height: number }) {
  await page.setViewportSize(viewport);
  await page.addInitScript((prefs: StoredPrefs) => window.localStorage.setItem(prefs.key, prefs.value), {
    key: PREFS_KEY,
    value: JSON.stringify({ theme }),
  });
  await installFakeStream(page);
  await mockApi(page, endpointReplies());
  await mockCaptureApi(page, ENDPOINT_ID, captureSet());
  await page.goto(CAPTURE_PATH);
  await expect(page.getByRole('heading', { name: 'Rules', exact: true })).toBeVisible();
}

for (const theme of THEMES) {
  test(`the run logs sections have no axe violations in the ${theme} theme at 360 and 1440`, async ({ page }) => {
    await openCapture(page, theme, MOBILE);
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);

    const narrow = await new AxeBuilder({ page }).analyze();
    expect(narrow.violations).toEqual([]);
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBe(
      0,
    );

    await page.setViewportSize(DESKTOP);
    const wide = await new AxeBuilder({ page }).analyze();
    expect(wide.violations).toEqual([]);
  });
}

test('a rule run pushed on the live stream replaces the queued state in the Rules section', async ({ page }) => {
  await openCapture(page, 'light', DESKTOP);
  await expect.poll(() => streamCount(page)).toBeGreaterThan(0);
  await openLatestStream(page);

  const captureId = captureIdFor(TARGET_SEQ);
  const queued = ruleRunFixture({
    id: RERUN_FIXTURE_ID,
    capture_id: captureId,
    trigger: 'rerun',
    state: 'queued',
    response_status: null,
    error: null,
    entries: [],
  });
  await pushStreamEvent(page, 'rule_result', { capture_id: captureId, run: queued });
  await expect(page.getByText('Queued', { exact: true })).toHaveCount(1);

  const finished = ruleRunFixture({
    ...queued,
    state: 'succeeded',
    response_status: 200,
    entries: runsOfTarget()[1]?.entries ?? [],
  });
  await pushStreamEvent(page, 'rule_result', { capture_id: captureId, run: finished });
  await expect(page.getByText('Queued', { exact: true })).toHaveCount(0);
  await expect(page.getByText('Succeeded', { exact: true })).toHaveCount(2);
});

test('an editor can run the rules again after confirming, and the new run is queued', async ({ page }) => {
  await openCapture(page, 'light', DESKTOP);
  let posts = 0;
  await page.route(`**${RERUN_PATH}`, async (route) => {
    if (route.request().method() !== 'POST') {
      await route.fallback();
      return;
    }
    posts += 1;
    const queued = ruleRunFixture({
      id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f6d01',
      capture_id: captureIdFor(TARGET_SEQ),
      trigger: 'rerun',
      state: 'queued',
      response_status: null,
      entries: [],
    });
    await route.fulfill({ status: 202, contentType: 'application/json', body: JSON.stringify(queued) });
  });

  await page.getByRole('button', { name: 'Run again', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Run again', exact: true }).click();

  await expect(page.getByText('Re-run queued.')).toBeVisible();
  await expect.poll(() => posts).toBe(1);
});
