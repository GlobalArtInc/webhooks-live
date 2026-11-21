import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page, type Route } from '@playwright/test';
import type { Rule, RuleList, RuleOrderInput } from '../src/api/rulesTypes';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { rulesFixture } from '../src/test/fixtures/rules';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const BASE_PATH = `/e/${ENDPOINT_ID}`;
const SECTION_PATH = `${BASE_PATH}/settings/rules`;
const RULES_PATH = `/api/v1/endpoints/${ENDPOINT_ID}/rules`;
const LIST_NAME = 'Rules in run order';
const WIDTHS = [
  { width: 360, height: 740 },
  { width: 1024, height: 768 },
  { width: 1440, height: 900 },
] as const;

function endpointReplies(): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}` },
    },
  };
}

interface RuleServer {
  list: RuleList;
}

interface RuleOptions {
  failOrder?: boolean;
}

interface RuleToggle {
  enabled: boolean;
}

async function serveRules(page: Page, rules: Rule[], options: RuleOptions = {}): Promise<RuleServer> {
  const server: RuleServer = { list: { items: rules, version: 1 } };
  await page.route(`**${RULES_PATH}**`, (route) => answerRules(route, server, options.failOrder === true));
  return server;
}

async function answerRules(route: Route, server: RuleServer, failOrder: boolean): Promise<void> {
  const request = route.request();
  const path = new URL(request.url()).pathname;
  const method = request.method();

  if (method === 'GET' && path === RULES_PATH) {
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(server.list) });
    return;
  }
  if (method === 'PUT' && path === `${RULES_PATH}/order`) {
    if (failOrder) {
      await route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: { code: 'internal_error', message: 'failed' } }),
      });
      return;
    }
    const body = request.postDataJSON() as RuleOrderInput;
    const byId = new Map(server.list.items.map((rule) => [rule.id, rule] as const));
    server.list = {
      items: body.ids.flatMap((id) => {
        const rule = byId.get(id);
        return rule === undefined ? [] : [rule];
      }),
      version: server.list.version + 1,
    };
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(server.list) });
    return;
  }
  if (method === 'PATCH' && path.startsWith(`${RULES_PATH}/`)) {
    const ruleId = path.slice(RULES_PATH.length + 1);
    const change = request.postDataJSON() as RuleToggle;
    server.list = {
      ...server.list,
      items: server.list.items.map((rule) => (rule.id === ruleId ? { ...rule, enabled: change.enabled } : rule)),
    };
    const updated = server.list.items.find((rule) => rule.id === ruleId);
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(updated) });
    return;
  }
  if (method === 'DELETE' && path.startsWith(`${RULES_PATH}/`)) {
    const ruleId = path.slice(RULES_PATH.length + 1);
    server.list = { ...server.list, items: server.list.items.filter((rule) => rule.id !== ruleId) };
    await route.fulfill({ status: 204 });
    return;
  }
  await route.fallback();
}

async function openRules(page: Page, rules: Rule[] = rulesFixture(), options: RuleOptions = {}): Promise<void> {
  await mockApi(page, endpointReplies());
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await installFakeStream(page);
  await serveRules(page, rules, options);
  await page.goto(SECTION_PATH);
}

function cardList(page: Page) {
  return page.getByRole('list', { name: LIST_NAME });
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

for (const { width, height } of WIDTHS) {
  test(`the rule list opens at ${width} px without sideways scrolling`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openRules(page);

    await expect(page.getByRole('heading', { level: 2, name: 'Rules' })).toBeVisible();
    await expect(cardList(page).getByRole('region')).toHaveCount(3);
    expect(await horizontalOverflow(page)).toBe(0);
  });
}

test('the rule list shows the empty state when there are no rules', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openRules(page, []);

  await expect(page.getByText('No rules yet')).toBeVisible();
});

test('a keyboard move is announced and the saved order survives a reload', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openRules(page);

  await page.getByRole('link', { name: 'Reject bad signature' }).press('Alt+Shift+ArrowUp');

  await expect(page.getByText('Reject bad signature moved to position 1 of 3.')).toBeAttached();
  await expect(page.getByRole('link', { name: 'Reject bad signature' })).toBeFocused();
  await page.reload();
  await expect(cardList(page).getByRole('region').first()).toHaveAccessibleName('Rule 1 of 3: Reject bad signature');
});

test('a failed order save puts the order back and shows an alert', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openRules(page, rulesFixture(), { failOrder: true });

  await page.getByRole('link', { name: 'Reject bad signature' }).press('Alt+Shift+ArrowUp');

  await expect(page.getByRole('alert')).toContainText('The order could not be saved. The previous order is back.');
  await expect(cardList(page).getByRole('region').first()).toHaveAccessibleName('Rule 1 of 3: Mark paid orders');
});

test('dragging a rule by its handle moves it to the top', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openRules(page);

  const card = page.getByRole('region', { name: 'Rule 3 of 3: Mail to chat' });
  const handle = card.locator('[aria-hidden="true"]', { hasText: '::' });
  const box = await handle.boundingBox();
  if (box === null) {
    throw new Error('the drag handle has no box');
  }
  const startX = box.x + box.width / 2;
  const startY = box.y + box.height / 2;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX, startY - 260, { steps: 20 });
  await page.mouse.up();

  await expect(cardList(page).getByRole('region').first()).toHaveAccessibleName('Rule 1 of 3: Mail to chat');
});

test('a switch turns a rule on at once, and deleting asks first', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openRules(page);

  await page.getByRole('switch', { name: 'Enabled: Mail to chat' }).click();
  await expect(page.getByRole('switch', { name: 'Enabled: Mail to chat' })).toHaveAttribute('aria-checked', 'true');

  await page.getByRole('button', { name: 'Delete: Mail to chat' }).click();
  const dialog = page.getByRole('dialog', { name: 'Delete rule?' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Delete rule' }).click();

  await expect(cardList(page).getByRole('region')).toHaveCount(2);
});

test('New rule opens the editor placeholder', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openRules(page);

  await page.getByRole('link', { name: 'New rule' }).click();

  await expect(page).toHaveURL(`${SECTION_PATH}/new?from=blank`);
  await expect(page.getByRole('heading', { level: 2, name: 'Rule editor' })).toBeVisible();
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the rule list has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openRules(page);
    await chooseTheme(page, theme);
    await expect(page.getByRole('heading', { level: 2, name: 'Rules' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
