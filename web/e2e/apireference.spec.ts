import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { SIGNED_IN_USER } from '../src/test/fixtures/usage';
import { instanceWith, mockApi, type Reply } from './support/mockApi';

const FIXTURE_PATH = fileURLToPath(new URL('../src/test/fixtures/openapi.json', import.meta.url));
const document: unknown = JSON.parse(readFileSync(FIXTURE_PATH, 'utf8'));
const OPERATION_COUNT = 94;
const WIDTHS = [
  { width: 360, height: 740 },
  { width: 1440, height: 900 },
] as const;

async function openReference(page: Page, path = '/docs/api', instance: Reply = instanceWith()): Promise<void> {
  await mockApi(page, {
    'GET /instance': instance,
    'GET /openapi.json': { body: document, headers: { 'Cache-Control': 'no-cache' } },
  });
  await page.goto(path);
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => window.document.documentElement.scrollWidth - window.document.documentElement.clientWidth);
}

test('the index shows the overview, the authentication schemes, the tags and every operation', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openReference(page);

  await expect(page.getByRole('heading', { level: 1, name: 'webhooks.live API' })).toBeVisible();
  await expect(page.getByText('Version 0.1.0-dev')).toBeVisible();
  await expect(page.getByRole('heading', { level: 2, name: 'Authentication' })).toBeVisible();
  await expect(page.getByRole('article')).toHaveCount(OPERATION_COUNT);
  await expect(page.getByRole('navigation', { name: 'API operations' }).getByRole('link')).toHaveCount(OPERATION_COUNT);
  await expect(page.getByRole('link', { name: 'Download OpenAPI' })).toHaveAttribute('href', '/api/v1/openapi.json');
});

test('the filter narrows the operations and is kept in the address', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openReference(page);
  await expect(page.getByRole('article')).toHaveCount(OPERATION_COUNT);

  await page.getByRole('searchbox', { name: 'Filter operations' }).fill('aliases_');

  await expect(page.getByRole('article')).toHaveCount(4);
  await expect(page).toHaveURL(/\/docs\/api\?q=aliases_/);
  await page.reload();
  await expect(page.getByRole('searchbox', { name: 'Filter operations' })).toHaveValue('aliases_');
  await expect(page.getByRole('article')).toHaveCount(4);
});

test('a deep link scrolls to its operation and focuses the heading', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openReference(page, '/docs/api/rules_create');

  const heading = page.getByRole('heading', { level: 3, name: /POST\s*\/endpoints\/\{id\}\/rules$/ });
  await expect(heading).toBeFocused();
  await expect(heading).toBeInViewport();
  await page
    .getByRole('navigation', { name: 'API operations' })
    .getByRole('link', { name: /GET\s*\/usage/ })
    .click();
  await expect(page).toHaveURL(/\/docs\/api\/usage_get$/);
  await expect(page.getByRole('heading', { level: 3, name: /GET\s*\/usage$/ })).toBeFocused();
});

test('copy curl puts the command with the token variable on the clipboard', async ({ page, context, baseURL }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: new URL(baseURL ?? '').origin });
  await page.setViewportSize({ width: 1440, height: 900 });
  await openReference(page, '/docs/api/endpoints_create');
  const article = page.getByRole('article', { name: /POST\s*\/endpoints$/ });

  await article.getByRole('button', { name: 'Copy the curl sample of POST /endpoints' }).click();

  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toBe(
      [
        'curl --request POST \\',
        `  --url '${new URL(baseURL ?? '').origin}/api/v1/endpoints' \\`,
        '  --header "Authorization: Bearer $WEBHOOKS_LIVE_TOKEN" \\',
        "  --header 'Content-Type: application/json' \\",
        "  --data '{}'",
      ].join('\n'),
    );
  await expect(article.getByRole('status').filter({ hasText: 'Copied to the clipboard.' })).toHaveCount(1);
});

test('Open in composer fills the method, the URL and the body', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openReference(page, '/docs/api/tokens_create');

  await page.getByRole('button', { name: 'Open in composer: POST /tokens' }).click();

  await expect(page).toHaveURL(/\/send$/);
  await expect(page.getByLabel('Method')).toHaveValue('POST');
  await expect(page.getByLabel('Target URL')).toHaveValue(/\/api\/v1\/tokens$/);
});

test('the account menu and the signed-out header both lead to the reference', async ({ page }) => {
  await openReference(page, '/settings', instanceWith({ auth_mode: 'mixed', user: SIGNED_IN_USER }));
  await page.getByRole('button', { name: /^Account: / }).click();
  await page.getByRole('menuitem', { name: 'API reference' }).click();
  await expect(page).toHaveURL(/\/docs\/api$/);
  await expect(page.getByRole('heading', { level: 1, name: 'webhooks.live API' })).toBeVisible();

  await page.unrouteAll();
  await openReference(page, '/settings');
  await page.getByRole('navigation', { name: 'Account links' }).getByRole('link', { name: 'API reference' }).click();
  await expect(page).toHaveURL(/\/docs\/api$/);
});

for (const { width, height } of WIDTHS) {
  test(`the reference fits at ${width} px without sideways scrolling`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openReference(page, '/docs/api/captures_list');

    await expect(page.getByRole('heading', { level: 3, name: /GET\s*\/endpoints\/\{id\}\/captures$/ })).toBeFocused();
    expect(await horizontalOverflow(page)).toBe(0);
    if (width < 1024) {
      await page.getByRole('button', { name: 'Operations' }).click();
      await expect(page.getByRole('navigation', { name: 'API operations' })).toBeVisible();
      expect(await horizontalOverflow(page)).toBe(0);
    }
  });

  for (const theme of ['Light', 'Dark'] as const) {
    test(`the reference has no axe violations at ${width} px in the ${theme.toLowerCase()} theme`, async ({ page }) => {
      await page.setViewportSize({ width, height });
      await openReference(page, '/docs/api/captures_list');
      await chooseTheme(page, theme);
      await expect(page.getByRole('article')).toHaveCount(OPERATION_COUNT);
      if (width < 1024) {
        await page.getByRole('button', { name: 'Operations' }).click();
      }

      const result = await new AxeBuilder({ page }).analyze();
      expect(result.violations).toEqual([]);
    });
  }
}
