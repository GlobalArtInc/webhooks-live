import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { PUBLIC_URL } from '../src/test/fixtures/instance';
import { captureIdFor, captureSeries } from '../src/features/endpoint/testFixtures';
import { mockCaptureApi, type CaptureApiState } from './support/captureApi';
import { ENDPOINT_ID, instanceWith, mockApi, type ReplyMap } from './support/mockApi';

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const LIST_NAME = 'Captures';
const DIALOG_NAME = 'Keyboard shortcuts';
const SWITCH_NAME = 'Single-key shortcuts';
const DESKTOP = { width: 1440, height: 900 };
const NARROW = { width: 360, height: 740 };

function captureAt(seq: number): string {
  return `${ENDPOINT_PATH}/c/${captureIdFor(seq)}`;
}

function endpointReplies(captureCount: number): ReplyMap {
  return {
    'GET /instance': instanceWith(),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: {
        ...endpointFixture,
        id: ENDPOINT_ID,
        url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
        capture_count: captureCount,
        last_seq: captureCount,
      },
    },
  };
}

async function openEndpoint(page: Page, captureCount: number, path: string = ENDPOINT_PATH): Promise<CaptureApiState> {
  await mockApi(page, endpointReplies(captureCount));
  const state = await mockCaptureApi(page, ENDPOINT_ID, captureSeries(captureCount));
  await page.goto(path);
  await expect(page.getByRole('listbox', { name: LIST_NAME })).toBeVisible();
  return state;
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document.getAnimations().filter((animation) => animation.effect?.getComputedTiming().endTime !== Infinity)
            .length,
      ),
    )
    .toBe(0);
}

function followToggle(page: Page) {
  return page.getByRole('button', { name: 'Follow', exact: true });
}

test.beforeEach(async ({ context, baseURL }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: new URL(baseURL ?? '').origin });
});

test('j and Down open the older capture, k and Up the newer one', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));

  await page.keyboard.press('j');
  await expect(page).toHaveURL(captureAt(2));
  await page.keyboard.press('ArrowDown');
  await expect(page).toHaveURL(captureAt(1));
  await page.keyboard.press('k');
  await expect(page).toHaveURL(captureAt(2));
  await page.keyboard.press('ArrowUp');
  await expect(page).toHaveURL(captureAt(3));
});

for (const key of ['Shift+K', 'Home'] as const) {
  test(`${key} opens the newest capture and turns follow on`, async ({ page }) => {
    await page.setViewportSize(DESKTOP);
    await openEndpoint(page, 5, captureAt(2));
    await expect(followToggle(page)).toHaveAttribute('aria-pressed', 'false');

    await page.keyboard.press(key);

    await expect(page).toHaveURL(captureAt(5));
    await expect(followToggle(page)).toHaveAttribute('aria-pressed', 'true');
  });
}

for (const key of ['Shift+J', 'End'] as const) {
  test(`${key} opens the oldest capture`, async ({ page }) => {
    await page.setViewportSize(DESKTOP);
    await openEndpoint(page, 5, captureAt(4));

    await page.keyboard.press(key);

    await expect(page).toHaveURL(captureAt(1));
  });
}

test('f toggles follow', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));
  await expect(followToggle(page)).toHaveAttribute('aria-pressed', 'false');

  await page.keyboard.press('f');
  await expect(followToggle(page)).toHaveAttribute('aria-pressed', 'true');
  await page.keyboard.press('f');
  await expect(followToggle(page)).toHaveAttribute('aria-pressed', 'false');
});

test('c copies the capture URL', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));

  await page.keyboard.press('c');

  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(`${PUBLIC_URL}/${ENDPOINT_ID}`);
});

test('y copies the selected capture as curl', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));
  await expect(page.getByRole('group', { name: 'Display mode' })).toBeVisible();

  await page.keyboard.press('y');

  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toMatch(/^curl .*\/hook\/3/);
});

test('r switches the selected capture body between pretty and raw', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));
  const mode = page.getByRole('group', { name: 'Display mode' });
  const pretty = mode.getByRole('button', { name: 'Pretty', exact: true });
  const raw = mode.getByRole('button', { name: 'Raw', exact: true });
  await expect(pretty).toHaveAttribute('aria-pressed', 'true');

  await page.keyboard.press('r');
  await expect(raw).toHaveAttribute('aria-pressed', 'true');
  await page.keyboard.press('r');
  await expect(pretty).toHaveAttribute('aria-pressed', 'true');
});

test('w toggles wrapping of the selected capture body', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));
  const wrap = page.getByRole('button', { name: 'Wrap', exact: true });
  await expect(wrap).toHaveAttribute('aria-pressed', 'false');

  await page.keyboard.press('w');
  await expect(wrap).toHaveAttribute('aria-pressed', 'true');
});

test('d marks the selected capture, then diffs the next capture with the marked one', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));

  const pane = page.getByRole('region', { name: 'Selected capture' });

  await page.keyboard.press('d');
  await expect(pane.getByRole('button', { name: 'Unmark for compare' })).toBeVisible();
  await page.keyboard.press('j');
  await expect(page).toHaveURL(captureAt(2));
  await expect(pane.getByRole('button', { name: 'Compare with marked' })).toBeVisible();
  await page.keyboard.press('d');

  await expect(page).toHaveURL(`${ENDPOINT_PATH}/diff/${captureIdFor(3)}/${captureIdFor(2)}`);
});

test('Delete removes the selected capture and offers Undo', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));
  const listbox = page.getByRole('listbox', { name: LIST_NAME });

  await page.keyboard.press('Delete');

  await expect(page.getByText('Capture deleted')).toBeVisible();
  await expect(listbox.getByRole('option')).toHaveCount(4);
});

test('s opens the endpoint settings', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5);

  await page.keyboard.press('s');

  await expect(page).toHaveURL(`${ENDPOINT_PATH}/settings`);
});

test('Shift+F opens the forwarding workspace', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5);

  await page.keyboard.press('Shift+F');

  await expect(page).toHaveURL(`${ENDPOINT_PATH}/forwarding`);
});

test('n opens the home page where a new endpoint is created', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5);

  await page.keyboard.press('n');

  await expect.poll(() => new URL(page.url()).pathname).toBe('/');
});

test('? opens the shortcuts dialog listing the table, and Esc closes it', async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5);
  const dialog = page.getByRole('dialog', { name: DIALOG_NAME });

  await page.keyboard.press('?');

  await expect(dialog).toBeVisible();
  await expect(dialog.getByText('Toggle follow')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
});

test('Esc leaves the capture detail on a 360 px screen', async ({ page }) => {
  await page.setViewportSize(NARROW);
  await openEndpoint(page, 20);

  await page.getByRole('listbox', { name: LIST_NAME }).getByRole('option').first().click();
  await expect(page).toHaveURL(new RegExp(`${ENDPOINT_PATH}/c/[0-9a-f-]{36}$`));
  await expect(page.getByRole('region', { name: 'Selected capture' })).toBeVisible();

  await page.keyboard.press('Escape');

  await expect(page).toHaveURL(ENDPOINT_PATH);
});

test('turning single-key shortcuts off keeps Esc and Down working, and the header button turns them back on', async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await openEndpoint(page, 5, captureAt(3));
  const dialog = page.getByRole('dialog', { name: DIALOG_NAME });

  await page.keyboard.press('?');
  await page.getByRole('switch', { name: SWITCH_NAME }).click();
  await expect(page.getByRole('switch', { name: SWITCH_NAME })).toHaveAttribute('aria-checked', 'false');
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();

  await page.keyboard.press('j');
  await page.keyboard.press('ArrowDown');
  await expect(page).toHaveURL(captureAt(2));

  await page.getByRole('button', { name: 'Shortcuts', exact: true }).click();
  await page.getByRole('switch', { name: SWITCH_NAME }).click();
  await expect(page.getByRole('switch', { name: SWITCH_NAME })).toHaveAttribute('aria-checked', 'true');
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();

  await page.keyboard.press('j');
  await expect(page).toHaveURL(captureAt(1));
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the shortcuts dialog has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize(DESKTOP);
    await openEndpoint(page, 5, captureAt(3));
    await chooseTheme(page, theme);
    await page.getByRole('button', { name: 'Shortcuts', exact: true }).click();
    await expect(page.getByRole('dialog', { name: DIALOG_NAME })).toBeVisible();

    const dialog = await new AxeBuilder({ page }).analyze();
    expect(dialog.violations).toEqual([]);
  });
}
