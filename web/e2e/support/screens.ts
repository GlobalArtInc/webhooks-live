import { expect, type Locator, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { captureIdFor, captureSeries } from '../../src/features/endpoint/testFixtures';
import { SIGNED_IN_USER, tokenFixture } from '../../src/features/tokens/testFixtures';
import { jsonCaptureFixture } from '../../src/test/fixtures/captures';
import { endpointFixture } from '../../src/test/fixtures/endpoint';
import { usageFixture } from '../../src/test/fixtures/general';
import { instanceFixture, PUBLIC_URL } from '../../src/test/fixtures/instance';
import { catalogueFixture, INVALID_TEMPLATE_BODY, templatedEndpoint } from '../../src/test/fixtures/templating';
import { mockCaptureApi } from './captureApi';
import { installFakeStream } from './fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './mockApi';
import { EDITOR_RULE_ID, mockRuleEditorApi } from './ruleEditorApi';

export interface ScreenSize {
  width: number;
  height: number;
}

export interface Screen {
  name: string;
  open: (page: Page) => Promise<void>;
}

export const WIDE: ScreenSize = { width: 1440, height: 900 };
export const MEDIUM: ScreenSize = { width: 1024, height: 768 };
export const NARROW: ScreenSize = { width: 360, height: 740 };

const OWNER = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21', email: 'owner@example.test', name: 'Owner' };
const WEEK_SECONDS = 604800;
const MONTH_SECONDS = 2592000;
const OPENAPI_PATH = fileURLToPath(new URL('../../src/test/fixtures/openapi.json', import.meta.url));

function endpointReplies(extra: ReplyMap = {}): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: {
        ...endpointFixture,
        id: ENDPOINT_ID,
        url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
        capture_count: 30,
        last_seq: 30,
      },
    },
    'GET /folders': { body: { items: [] } },
    [`GET /endpoints/${ENDPOINT_ID}/aliases`]: { body: { items: [] } },
    [`GET /endpoints/${ENDPOINT_ID}/template/variables`]: { body: catalogueFixture },
    ...extra,
  };
}

async function openEndpointScreen(page: Page, path: string, ready: Locator): Promise<void> {
  await installFakeStream(page);
  await mockApi(page, endpointReplies());
  await mockCaptureApi(page, ENDPOINT_ID, captureSeries(30));
  await page.goto(path);
  await expect(ready).toBeVisible();
}

async function openHome(page: Page): Promise<void> {
  await mockApi(page, { 'GET /instance': instanceWith() });
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
}

async function openList(page: Page): Promise<void> {
  await openEndpointScreen(page, `/e/${ENDPOINT_ID}`, page.getByRole('listbox', { name: 'Captures' }));
  await expect(page.getByRole('listbox', { name: 'Captures' }).getByRole('option').first()).toBeVisible();
}

async function openDetail(page: Page): Promise<void> {
  await openEndpointScreen(
    page,
    `/e/${ENDPOINT_ID}/c/${captureIdFor(5)}`,
    page.getByRole('heading', { name: /^Capture / }),
  );
  await expect(page.locator('main .cm-editor').first()).toBeVisible();
}

async function openGeneral(page: Page): Promise<void> {
  await installFakeStream(page);
  await mockApi(
    page,
    endpointReplies({
      'GET /instance': instanceWith({
        auth_mode: 'mixed',
        user: OWNER,
        limits: {
          ...instanceFixture.limits,
          retention_seconds: WEEK_SECONDS,
          max_retention_seconds: MONTH_SECONDS,
          never_expire_endpoints: 3,
          max_captures_settable: 500,
        },
      }),
      [`GET /endpoints/${ENDPOINT_ID}`]: {
        body: {
          ...endpointFixture,
          id: ENDPOINT_ID,
          url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
          my_role: 'owner',
          owner: OWNER,
          max_captures: 500,
          retention: { mode: 'sliding', seconds: null },
          capture_ttl_seconds: null,
          sender_allow_cidrs: [],
          capture_auth: null,
          verifier: null,
          view_protected: false,
        },
      },
      'GET /usage': { body: usageFixture },
    }),
  );
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await page.goto(`/e/${ENDPOINT_ID}/settings/general`);
  await expect(page.getByRole('heading', { level: 2, name: 'General' })).toBeVisible();
}

async function openResponse(page: Page): Promise<void> {
  await installFakeStream(page);
  await mockApi(
    page,
    endpointReplies({
      [`GET /endpoints/${ENDPOINT_ID}`]: {
        body: { ...templatedEndpoint(INVALID_TEMPLATE_BODY), id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}` },
      },
    }),
  );
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await page.goto(`/e/${ENDPOINT_ID}/settings/response`);
  await expect(page.getByRole('heading', { level: 2, name: 'Response' })).toBeVisible();
  await expect(page.locator('main .cm-editor').first()).toBeVisible();
}

async function openRules(page: Page): Promise<void> {
  await mockRuleEditorApi(page);
  await page.goto(`/e/${ENDPOINT_ID}/settings/rules`);
  await expect(page.getByRole('heading', { level: 2, name: 'Rules' })).toBeVisible();
}

async function openRuleEditor(page: Page): Promise<void> {
  await mockRuleEditorApi(page);
  await page.goto(`/e/${ENDPOINT_ID}/settings/rules/${EDITOR_RULE_ID}`);
  await expect(page.getByRole('textbox', { name: 'Rule name' })).toBeVisible();
}

async function openComposer(page: Page): Promise<void> {
  await mockApi(page, {
    'GET /instance': instanceWith(),
    'GET /send/history': { body: { items: [], next_cursor: null } },
  });
  await mockCaptureApi(page, ENDPOINT_ID, [jsonCaptureFixture]);
  await page.goto(`/send?endpoint=${ENDPOINT_ID}&capture=${jsonCaptureFixture.id}`);
  await expect(page.getByLabel('Target URL')).toBeVisible();
}

async function openReference(page: Page): Promise<void> {
  const document: unknown = JSON.parse(readFileSync(OPENAPI_PATH, 'utf8'));
  await mockApi(page, {
    'GET /instance': instanceWith(),
    'GET /openapi.json': { body: document, headers: { 'Cache-Control': 'no-cache' } },
  });
  await page.goto('/docs/api');
  await expect(page.getByRole('heading', { level: 1, name: 'webhooks.live API' })).toBeVisible();
}

async function openTokens(page: Page): Promise<void> {
  await mockApi(page, {
    'GET /instance': instanceWith({ auth_mode: 'mixed', user: SIGNED_IN_USER }),
    'GET /endpoints': { body: { items: [{ ...endpointFixture, id: ENDPOINT_ID }], next_cursor: null } },
  });
  await page.route('**/api/v1/tokens**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: [tokenFixture()] }),
    }),
  );
  await page.goto('/settings/tokens');
  await expect(page.getByRole('heading', { level: 1, name: 'API tokens' })).toBeVisible();
  await expect(page.getByText(tokenFixture().name).first()).toBeVisible();
}

export const SCREENS: Screen[] = [
  { name: 'home', open: openHome },
  { name: 'list', open: openList },
  { name: 'detail', open: openDetail },
  { name: 'general', open: openGeneral },
  { name: 'response', open: openResponse },
  { name: 'rules', open: openRules },
  { name: 'rule-editor', open: openRuleEditor },
  { name: 'composer', open: openComposer },
  { name: 'api-reference', open: openReference },
  { name: 'tokens', open: openTokens },
];
