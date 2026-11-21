import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { instanceWith, mockApi } from './support/mockApi';

const USER = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21', email: 'ada@example.test', name: 'Ada Lovelace' };
const SIGNED_IN = instanceWith({
  auth_mode: 'sso',
  user: USER,
  auth: { provider_name: 'Example ID', sign_in_available: true },
});
const OPEN = instanceWith();
const WIDTHS = [
  { width: 360, height: 740 },
  { width: 1024, height: 768 },
] as const;

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page
    .getByRole('main')
    .getByRole('radiogroup', { name: 'Theme' })
    .getByRole('radio', { name: theme, exact: true })
    .check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test('the account menu opens from the keyboard and links to the account page', async ({ page }) => {
  await mockApi(page, { 'GET /instance': SIGNED_IN });
  await page.goto('/');

  await page.getByRole('button', { name: 'Account: Ada Lovelace' }).focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('menuitem', { name: 'Sign out' })).toBeVisible();
  await page.getByRole('menuitem', { name: 'Account', exact: true }).click();

  await expect(page).toHaveURL('/account');
  await expect(page.getByRole('heading', { level: 1, name: 'Account' })).toBeVisible();
  await expect(page.getByText('ada@example.test')).toBeVisible();
  await expect(page.getByText('Example ID')).toBeVisible();
});

test('sign out from the account menu posts to the logout endpoint and returns home', async ({ page }) => {
  await mockApi(page, { 'GET /instance': SIGNED_IN });
  await page.goto('/');

  await page.getByRole('button', { name: 'Account: Ada Lovelace' }).click();
  const logout = page.waitForRequest(
    (request) => request.method() === 'POST' && request.url().endsWith('/auth/logout'),
  );
  await page.getByRole('menuitem', { name: 'Sign out' }).click();
  await logout;
  await expect(page).toHaveURL('/');
});

test('the sign out button on the account page posts to the logout endpoint and returns home', async ({ page }) => {
  await mockApi(page, { 'GET /instance': SIGNED_IN });
  await page.goto('/account');

  const logout = page.waitForRequest(
    (request) => request.method() === 'POST' && request.url().endsWith('/auth/logout'),
  );
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await logout;
  await expect(page).toHaveURL('/');
});

test('in open mode the header keeps Preferences, offers no account menu, and the account page says so', async ({
  page,
}) => {
  await mockApi(page, { 'GET /instance': OPEN });
  await page.goto('/');

  const links = page.getByRole('navigation', { name: 'Account links' });
  await expect(links.getByRole('link', { name: 'Preferences' })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Account:/ })).toHaveCount(0);
  await expect(links.getByRole('link', { name: 'Sign in' })).toHaveCount(0);
  await links.getByRole('link', { name: 'Preferences' }).click();
  await expect(page).toHaveURL('/settings');
  await expect(page.getByRole('heading', { level: 1, name: 'Preferences' })).toBeVisible();

  await page.goto('/account');
  await expect(page.getByRole('heading', { level: 1, name: 'Accounts are not used on this instance' })).toBeVisible();
});

test('preferences apply without a reload and survive one', async ({ page }) => {
  await mockApi(page, { 'GET /instance': OPEN });
  await page.goto('/settings');
  const main = page.getByRole('main');
  const follow = main.getByRole('switch', { name: 'Follow new captures by default' });

  await main.getByRole('radiogroup', { name: 'Density' }).getByRole('radio', { name: 'Compact' }).check();
  await expect(page.locator('html')).toHaveClass(/density-compact/);
  await chooseTheme(page, 'Dark');
  await follow.click();
  await expect(follow).toHaveAttribute('aria-checked', 'true');

  await page.reload();
  await expect(page.locator('html')).toHaveClass(/density-compact/);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(main.getByRole('radiogroup', { name: 'Density' }).getByRole('radio', { name: 'Compact' })).toBeChecked();
  await expect(main.getByRole('switch', { name: 'Follow new captures by default' })).toHaveAttribute(
    'aria-checked',
    'true',
  );

  await main.getByRole('button', { name: 'Reset all preferences' }).click();
  await expect(page.locator('html')).not.toHaveClass(/density-compact/);
  await expect(page.locator('html')).not.toHaveAttribute('data-theme');
  await expect(main.getByRole('status')).toHaveText('Preferences were reset to their defaults.');
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the preferences page has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await mockApi(page, { 'GET /instance': OPEN });
    await page.goto('/settings');
    await chooseTheme(page, theme);
    await expect(page.getByRole('heading', { level: 1, name: 'Preferences' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the account page has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await mockApi(page, { 'GET /instance': SIGNED_IN });
    await page.goto('/settings');
    await chooseTheme(page, theme);
    await page.goto('/account');
    await expect(page.getByRole('heading', { level: 1, name: 'Account' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}

for (const { width, height } of WIDTHS) {
  test(`the preferences, the account page and the menu fit at ${width} px without sideways scrolling`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await mockApi(page, { 'GET /instance': SIGNED_IN });
    await page.goto('/settings');
    await expect(page.getByRole('heading', { level: 1, name: 'Preferences' })).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);

    await page.goto('/account');
    await expect(page.getByRole('heading', { level: 1, name: 'Account' })).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);

    await page.getByRole('button', { name: 'Account: Ada Lovelace' }).click();
    await expect(page.getByRole('menuitem', { name: 'Preferences' })).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);
  });
}
