import { expect, test } from '@playwright/test';
import { ENDPOINT_ID, apiError, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/endpoints/${ENDPOINT_ID}`;

function openReplies(extra: ReplyMap = {}): ReplyMap {
  return { 'GET /instance': instanceWith(), ...extra };
}

test('the shell exposes a banner, a main landmark, and a skip link that moves focus to main', async ({ page }) => {
  await mockApi(page, openReplies());
  await page.goto('/');

  await expect(page.getByRole('banner')).toBeVisible();
  await expect(page.getByRole('main')).toBeVisible();

  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Skip to main content' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('main')).toBeFocused();
});

test('the offline banner appears while the network is down and clears when it returns', async ({ page, context }) => {
  await mockApi(page, openReplies());
  await page.goto('/');
  await expect(page.getByRole('banner')).toBeVisible();

  const message = 'You are offline. Shown data may be out of date.';
  await context.setOffline(true);
  await expect(page.getByText(message)).toBeVisible();

  await context.setOffline(false);
  await expect(page.getByText(message)).toHaveCount(0);
});

test('an anonymous visitor on an SSO instance sees the sign-in gate with the return path', async ({ page }) => {
  await mockApi(
    page,
    openReplies({
      'GET /instance': instanceWith({
        auth_mode: 'sso',
        auth: { provider_name: 'Example ID', sign_in_available: true },
      }),
    }),
  );
  await page.goto('/endpoints');

  const signIn = page.getByRole('link', { name: 'Sign in with Example ID' });
  await expect(signIn).toBeVisible();
  await expect(signIn).toHaveAttribute('href', '/auth/login?return_to=%2Fendpoints');
});

test('a 401 on an endpoint maps to the sign-in state', async ({ page }) => {
  await mockApi(page, openReplies({ [`GET ${ENDPOINT_PATH}`]: apiError(401, 'unauthenticated', 'sign in') }));
  await page.goto(`/e/${ENDPOINT_ID}`);

  await expect(page.getByRole('heading', { level: 1, name: 'Sign in to continue' })).toBeVisible();
});

test('a 403 on an endpoint shows the not-shared state and signs out through the logout endpoint', async ({ page }) => {
  const user = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21', email: 'viewer@example.test', name: null };
  await mockApi(
    page,
    openReplies({
      'GET /instance': instanceWith({ user }),
      [`GET ${ENDPOINT_PATH}`]: apiError(403, 'forbidden', 'not shared'),
    }),
  );
  await page.goto(`/e/${ENDPOINT_ID}`);

  await expect(page.getByRole('heading', { level: 1, name: 'This endpoint is not shared with you' })).toBeVisible();
  await expect(page.getByText('Signed in as viewer@example.test')).toBeVisible();

  const logout = page.waitForRequest(
    (request) => request.method() === 'POST' && request.url().endsWith('/auth/logout'),
  );
  await page.getByRole('button', { name: 'Sign out' }).click();
  await logout;
  await expect(page).toHaveURL('/');
});

test('an unknown path shows the not-found state inside the shell', async ({ page }) => {
  await mockApi(page, openReplies());
  await page.goto('/no/such/page');

  await expect(page.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
  await expect(page.getByRole('banner')).toBeVisible();
});

test('a failing instance shows the error state, and Retry recovers once the server answers', async ({ page }) => {
  const replies = openReplies({ 'GET /instance': apiError(500, 'internal', 'boom') });
  await mockApi(page, replies);
  await page.goto('/');

  const errorHeading = page.getByRole('heading', { level: 1, name: 'This page could not be loaded' });
  await expect(errorHeading).toBeVisible({ timeout: 20_000 });

  replies['GET /instance'] = instanceWith();
  await page.getByRole('button', { name: 'Retry' }).click();
  await expect(errorHeading).toHaveCount(0);
  await expect(page.getByRole('banner')).toBeVisible();
});
