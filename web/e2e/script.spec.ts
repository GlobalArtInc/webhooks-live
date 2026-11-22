import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import { SCRIPT_RECORDED_SOURCE } from '../src/features/rules/script/fixtures';
import { SLOW_MARKER, STALE_MESSAGE, mockScriptApi, type ScriptApiState } from './support/scriptApi';

const STEP_LABEL = 'Script';
const RUN_LABEL = 'Recorded script';
const LIMITS = /runs stop after 100 ms or 100,000 operations\./;

interface Opened {
  api: ScriptApiState;
  editor: Locator;
}

async function openSample(page: Page): Promise<Opened> {
  const api = await mockScriptApi(page);
  await page.goto('/dev/script');
  await expect(page.getByRole('heading', { level: 1, name: 'Script step editor' })).toBeVisible();
  const editor = page.getByRole('textbox', { name: STEP_LABEL, exact: true });
  await expect(editor).toBeVisible();
  await expect(page.getByText(LIMITS)).toBeVisible();
  return { api, editor };
}

async function chooseVariant(page: Page, name: 'Step card' | 'Full height rule' | 'Recorded run'): Promise<void> {
  await page.getByRole('radio', { name, exact: true }).check();
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test.describe('script editor', () => {
  test('a syntax error from the server marks its line, lists it with Go, and clears after the fix', async ({
    page,
  }) => {
    const { editor } = await openSample(page);
    const section = page.getByRole('region', { name: 'Step card' });

    await editor.click();
    await page.keyboard.press('ControlOrMeta+End');
    await page.keyboard.type(' +');

    await expect(section.getByText('Errors 1', { exact: true })).toBeVisible();
    await expect(section.locator('.cm-lintRange-error').first()).toBeVisible();
    await expect(section.locator('.cm-lint-marker-error').first()).toBeVisible();

    await section.getByRole('button', { name: /^Go to line 8, column 3: Expecting an expression/ }).click();
    await expect(editor).toBeFocused();

    await editor.click();
    await page.keyboard.press('ControlOrMeta+End');
    await page.keyboard.press('Backspace');
    await page.keyboard.press('Backspace');
    await expect(section.getByText('Errors 1', { exact: true })).toHaveCount(0);
    await expect(section.locator('.cm-lintRange-error')).toHaveCount(0);
  });

  test('a diagnostic in the exact shape the server answers marks its line and offers Go', async ({ page }) => {
    const { editor } = await openSample(page);
    const section = page.getByRole('region', { name: 'Step card' });
    await page.route('**/api/v1/script/validate', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          diagnostics: [
            { severity: 'error', code: 'syntax', message: "Unexpected ';'", line: 2, column: 9, length: 1 },
          ],
        }),
      }),
    );

    await editor.fill('let a = 1;\nlet b = ;');

    await expect(section.getByText('Errors 1', { exact: true })).toBeVisible();
    await expect(section.locator('.cm-lintRange-error').first()).toBeVisible();
    await expect(section.getByRole('button', { name: "Go to line 2, column 9: Unexpected ';'" })).toBeVisible();
  });

  test('completion lists the catalogue functions from the server meta and inserts a call', async ({ page }) => {
    const { editor } = await openSample(page);

    await editor.click();
    await page.keyboard.press('ControlOrMeta+End');
    await page.keyboard.press('Enter');
    await page.keyboard.type('hmac');

    const option = page.getByRole('option', { name: /hmac_sha256/ });
    await expect(option).toBeVisible();
    await option.click();
    await expect(editor).toContainText('hmac_sha256(');
  });

  test('completion stays closed after an operator and opens on Ctrl+Space', async ({ page }) => {
    const { editor } = await openSample(page);
    const section = page.getByRole('region', { name: 'Step card' });
    await page.route('**/api/v1/script/validate', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          diagnostics: [
            { severity: 'error', code: 'syntax', message: "Unexpected ';'", line: 1, column: 9, length: 1 },
          ],
        }),
      }),
    );

    await editor.click();
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.press('Backspace');
    await page.keyboard.type('let a = ;');
    await expect(section.getByText('Errors 1', { exact: true })).toBeVisible();
    await expect(page.locator('.cm-tooltip-autocomplete')).toHaveCount(0);

    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('Control+Space');
    await expect(page.getByRole('option', { name: /hmac_sha256/ })).toBeVisible();
  });

  test('hovering a function or a variable shows its documentation', async ({ page }) => {
    const { editor } = await openSample(page);

    await editor.click();
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.press('Backspace');
    await page.keyboard.type('sha256(request.path)');
    await page.keyboard.press('Enter');
    await page.keyboard.type('request.body');
    const lines = page.locator('.cm-line');
    await lines.filter({ hasText: 'sha256(' }).hover({ position: { x: 20, y: 8 } });
    await expect(page.locator('.cm-tooltip', { hasText: 'sha256(text: string) -> string' })).toBeVisible();
    await expect(page.locator('.cm-tooltip', { hasText: 'SHA-256 digest' })).toBeVisible();

    await lines.filter({ hasText: /^request\.body$/ }).hover({ position: { x: 20, y: 8 } });
    await expect(page.locator('.cm-tooltip', { hasText: 'request: map' })).toBeVisible();
  });

  test('a stale answer never replaces the diagnostics of the newer text', async ({ page }) => {
    const { api, editor } = await openSample(page);

    await editor.fill(`let a = ${SLOW_MARKER};`);
    await expect.poll(() => api.sources.filter((source) => source.includes(SLOW_MARKER)).length).toBe(1);
    await editor.fill('let a = 2;');
    await expect.poll(() => api.sources.at(-1)).toBe('let a = 2;');
    await expect.poll(() => api.released.length).toBe(1);
    await page.evaluate(
      () => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))),
    );

    await expect(page.getByText(STALE_MESSAGE)).toHaveCount(0);
    await expect(page.getByText('Errors 1', { exact: true })).toHaveCount(0);
  });

  test('typing quickly sends one request for the final text', async ({ page }) => {
    const { api, editor } = await openSample(page);
    await expect.poll(() => api.sources.length).toBeGreaterThan(0);
    const before = api.sources.length;

    await editor.click();
    await page.keyboard.press('ControlOrMeta+End');
    await page.keyboard.type(' // note', { delay: 20 });

    await expect.poll(() => api.sources.length).toBe(before + 1);
    expect(api.sources.at(-1)).toContain('// note');
  });

  test('a failed language load shows a notice, and Retry brings the limits back', async ({ page }) => {
    const api = await mockScriptApi(page);
    api.metaFails = true;
    await page.goto('/dev/script');

    await expect(page.getByText('Language help could not load. Completion and hover docs are off.')).toBeVisible();
    await expect(page.getByText(LIMITS)).toHaveCount(0);
    await expect(page.getByRole('textbox', { name: STEP_LABEL, exact: true })).toBeVisible();

    api.metaFails = false;
    await page.getByRole('button', { name: 'Retry' }).click();
    await expect(page.getByText(LIMITS)).toBeVisible();
    await expect(page.getByRole('link', { name: /Language reference/ })).toHaveAttribute('target', '_blank');
  });

  test('the recorded run is read-only, never asks the server, and jumps to the runtime error', async ({ page }) => {
    const { api } = await openSample(page);
    await chooseVariant(page, 'Recorded run');

    const recorded = page.getByRole('textbox', { name: RUN_LABEL, exact: true });
    await expect(recorded).toHaveAttribute('aria-readonly', 'true');
    await expect(page.getByText('Recorded run: the script is shown as it ran')).toBeVisible();
    await expect(page.getByText('Errors 1', { exact: true })).toBeVisible();
    await expect(page.getByText('Esc then Tab leaves the editor')).toHaveCount(0);
    expect(api.sources).not.toContain(SCRIPT_RECORDED_SOURCE);

    await page.getByRole('button', { name: 'Go to the runtime error' }).click();
    await expect(recorded).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(recorded).not.toBeFocused();
  });

  test('Esc then Tab leaves the script editor and Tab alone indents', async ({ page }) => {
    const { editor } = await openSample(page);

    await editor.click();
    await page.keyboard.press('ControlOrMeta+Home');
    await page.keyboard.press('Tab');
    await expect(editor).toBeFocused();
    await expect(editor).toContainText('  // Refuse');

    await page.keyboard.press('Escape');
    await expect(page.getByText('Press Tab to leave the editor')).toBeVisible();
    await page.keyboard.press('Tab');
    await expect(editor).not.toBeFocused();
  });

  test('the full-height variant fills the viewport and shows the limits after the response', async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await openSample(page);
    await chooseVariant(page, 'Full height rule');

    await expect(page.getByText('After the response:')).toBeVisible();
    await expect(page.getByText(/runs stop after 1 sec or 1,000,000 operations\./)).toBeVisible();
    await expect
      .poll(async () => (await page.locator('.cm-editor').first().boundingBox())?.height ?? 0)
      .toBeGreaterThan(450);
  });
});

test('the script editor fits a 360 px viewport in every variant without horizontal scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await openSample(page);

  for (const name of ['Step card', 'Full height rule', 'Recorded run'] as const) {
    await chooseVariant(page, name);
    await expect(page.getByRole('region', { name })).toBeVisible();
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
  }
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the script editor has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    const { editor } = await openSample(page);
    await chooseTheme(page, theme);
    await editor.click();
    await page.keyboard.press('ControlOrMeta+End');
    await page.keyboard.type(' +');
    await expect(page.getByText('Errors 1', { exact: true })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('listbox', { name: 'Completions' })).toHaveCount(0);

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the recorded run has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await openSample(page);
    await chooseTheme(page, theme);
    await chooseVariant(page, 'Recorded run');
    await expect(page.getByText('Errors 1', { exact: true })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}
