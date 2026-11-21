import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { ENDPOINT_ID, apiError, instanceWith, mockApi, type Reply, type ReplyMap } from './support/mockApi';

const SHORT_ID = '012345...npq';

const CREATED: Reply = {
  status: 201,
  body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}`, title: null },
};

function homeReplies(extra: ReplyMap = {}): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    'POST /endpoints': CREATED,
    [`GET /endpoints/${ENDPOINT_ID}`]: { body: CREATED.body },
    ...extra,
  };
}

async function createByEnter(page: Page): Promise<void> {
  await page.goto('/');
  await page.getByRole('button', { name: 'Create endpoint' }).focus();
  await expect(page.getByRole('button', { name: 'Create endpoint' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(`/e/${ENDPOINT_ID}`);
}

test('Enter on the focused create button creates an endpoint and opens its page', async ({ page }) => {
  await mockApi(page, homeReplies());

  await createByEnter(page);

  await expect(page.getByRole('heading', { level: 1, name: 'Endpoint' })).toBeVisible();
});

test('the list of my endpoints survives a reload', async ({ page }) => {
  await mockApi(page, homeReplies());
  await createByEnter(page);

  await page.goto('/');
  await expect(page.getByRole('main').getByText(SHORT_ID)).toBeVisible();

  await page.reload();
  await expect(page.getByRole('main').getByText(SHORT_ID)).toBeVisible();
});

test('a label edited inline is still there after a reload', async ({ page }) => {
  await mockApi(page, homeReplies());
  await createByEnter(page);
  await page.goto('/');

  await page.getByRole('button', { name: `Rename ${SHORT_ID}` }).click();
  await page.getByLabel(`Label for ${SHORT_ID}`).fill('stripe-dev');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('main').getByText('stripe-dev')).toBeVisible();

  await page.reload();
  await expect(page.getByRole('main').getByText('stripe-dev')).toBeVisible();
});

test('a 422 from the server marks the field it names and moves focus there', async ({ page }) => {
  await mockApi(
    page,
    homeReplies({
      'POST /endpoints': apiError(422, 'validation_failed', 'response.status must be between 100 and 599', [
        { field: 'response.status', message: 'must be between 100 and 599' },
      ]),
    }),
  );
  await page.goto('/');

  await page.getByRole('button', { name: 'Custom response' }).click();
  await page.getByRole('button', { name: 'Create with this response' }).click();

  const status = page.getByLabel('Status code');
  await expect(status).toHaveAttribute('aria-invalid', 'true');
  await expect(status).toBeFocused();
  await expect(page.getByText('This value is not valid.')).toBeVisible();
});

test('on a narrow screen the rail opens as a drawer from the Menu button and closes on choice', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await mockApi(page, homeReplies());
  await createByEnter(page);
  await page.goto('/');

  const newButton = page.getByRole('button', { name: 'New endpoint' });
  await expect(newButton).toBeHidden();
  await page.getByRole('button', { name: 'Menu' }).click();
  await expect(newButton).toBeVisible();

  await page.getByRole('navigation', { name: 'My endpoints' }).getByRole('link', { name: SHORT_ID }).click();
  await expect(page).toHaveURL(`/e/${ENDPOINT_ID}`);
  await expect(newButton).toBeHidden();
});

test('a gone endpoint offers removal from my endpoints, which empties the list', async ({ page }) => {
  await mockApi(page, homeReplies());
  await createByEnter(page);
  await mockApi(page, homeReplies({ [`GET /endpoints/${ENDPOINT_ID}`]: apiError(410, 'endpoint_gone', 'gone') }));
  await page.goto(`/e/${ENDPOINT_ID}`);

  await expect(page.getByRole('heading', { level: 1, name: 'This endpoint is gone' })).toBeVisible();
  await page.getByRole('button', { name: 'Remove from my endpoints' }).click();

  await page.goto('/');
  await expect(page.getByRole('main').getByText('No endpoints yet.', { exact: false })).toBeVisible();
});

test('the rail collapses to 56 px on a wide screen and the Menu button stays hidden', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockApi(page, homeReplies());
  await createByEnter(page);
  await page.goto('/');

  await expect(page.getByRole('button', { name: 'Menu' })).toBeHidden();
  await page.getByRole('navigation', { name: 'My endpoints' }).getByRole('button', { name: 'Collapse' }).click();

  const railWidth = () =>
    page
      .getByRole('navigation', { name: 'My endpoints' })
      .evaluate((element) => element.closest('aside')?.getBoundingClientRect().width ?? 0);
  await expect.poll(railWidth).toBe(56);
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the home screen has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await mockApi(page, homeReplies());
    await createByEnter(page);
    await page.goto('/');
    await page.getByRole('radio', { name: theme, exact: true }).check();
    await expect(page.getByRole('main').getByText(SHORT_ID)).toBeVisible();

    const home = await new AxeBuilder({ page }).analyze();
    expect(home.violations).toEqual([]);

    await page.getByRole('button', { name: 'Custom response' }).click();
    const withForm = await new AxeBuilder({ page }).analyze();
    expect(withForm.violations).toEqual([]);
  });
}
