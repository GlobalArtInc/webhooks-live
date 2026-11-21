import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

const BODY_LABEL = 'Response body (template, JSON)';
const SCRIPT_LABEL = 'Script';

async function openSample(page: Page): Promise<void> {
  await page.goto('/dev/editors');
  await expect(page.getByRole('heading', { level: 1, name: 'Editors kit' })).toBeVisible();
  await expect(page.getByRole('textbox', { name: BODY_LABEL, exact: true })).toBeVisible();
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test.describe('code editor', () => {
  test('underlines a server diagnostic, lists it with Go, and clears it after the fix', async ({ page }) => {
    await openSample(page);
    const body = page.getByRole('textbox', { name: BODY_LABEL, exact: true });
    const bodySection = page.getByRole('region', { name: BODY_LABEL });
    const errors = bodySection.getByText('Errors 1', { exact: true });

    await expect(errors).toBeVisible();
    await expect(bodySection.getByText('Warnings 1', { exact: true })).toBeVisible();
    await expect(bodySection.locator('.cm-lintRange-error').first()).toBeVisible();
    await expect(bodySection.locator('.cm-lint-marker-error').first()).toBeVisible();

    await page.getByRole('button', { name: /^Go to line 4, column 19: Unknown filter/ }).click();
    await expect(body).toBeFocused();

    await body.fill('{}');
    await expect(bodySection.getByText('Errors 1', { exact: true })).toHaveCount(0);
    await expect(bodySection.locator('.cm-lintRange-error')).toHaveCount(0);
  });

  test('Esc then Tab leaves the editor, while Tab alone indents', async ({ page }) => {
    await openSample(page);
    const script = page.getByRole('textbox', { name: SCRIPT_LABEL, exact: true });

    await script.click();
    await page.keyboard.press('ControlOrMeta+Home');
    await page.keyboard.press('Tab');
    await expect(script).toBeFocused();
    await expect(script).toContainText('  // Reply');

    await page.keyboard.press('Escape');
    await expect(page.getByText('Press Tab to leave the editor')).toBeVisible();
    await page.keyboard.press('Tab');
    await expect(script).not.toBeFocused();
  });

  test('the read-only editor does not trap Tab and refuses typing', async ({ page }) => {
    await openSample(page);
    const readOnly = page.getByRole('textbox', { name: 'Read-only body', exact: true });

    await readOnly.focus();
    await page.keyboard.type('x');
    await expect(readOnly).not.toContainText('x');
    await page.keyboard.press('Tab');
    await expect(readOnly).not.toBeFocused();
  });

  test('the single-line variant refuses new lines and Tab moves on', async ({ page }) => {
    await openSample(page);
    const header = page.getByRole('textbox', { name: 'Row 3: Value', exact: true });

    await header.click();
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await page.keyboard.type('!');
    await expect(header).toContainText('static!');
    await expect(page.locator('.cm-line').filter({ hasText: '!' })).toHaveCount(1);
    await page.keyboard.press('Tab');
    await expect(header).not.toBeFocused();
  });

  test('typing the template opener opens variable completion and Enter inserts the path', async ({ page }) => {
    await openSample(page);
    const body = page.getByRole('textbox', { name: BODY_LABEL, exact: true });

    await body.click();
    await page.keyboard.press('ControlOrMeta+End');
    await page.keyboard.type('{{');
    const option = page.getByRole('option', { name: /json\.data\.amount/ });
    await expect(option).toBeVisible();
    await page.keyboard.type('data.am');
    await expect(page.getByRole('option', { name: /json\.id/ })).toHaveCount(0);
    await option.click();
    await expect(body).toContainText('{{json.data.amount');
  });

  test('Mod+Enter submits from inside the editor', async ({ page }) => {
    await openSample(page);
    await page.getByRole('textbox', { name: BODY_LABEL, exact: true }).click();
    await page.keyboard.press('ControlOrMeta+Enter');
    await expect(page.getByText('preview requested 1 times')).toBeVisible();
  });
});

test.describe('pickers and lists', () => {
  test('the variable picker filters, inserts at the cursor and returns focus to the editor', async ({ page }) => {
    await openSample(page);
    const body = page.getByRole('textbox', { name: BODY_LABEL, exact: true });

    await body.click();
    await page.keyboard.press('ControlOrMeta+End');
    await page.getByRole('button', { name: 'Insert variable' }).first().click();
    await page.getByRole('combobox', { name: 'Filter variables' }).fill('amount');
    await expect(page.getByRole('listbox', { name: 'Variables' }).getByRole('option')).toHaveCount(2);
    await page.keyboard.press('Enter');

    await expect(body).toContainText('{{ request.json.data.amount }}');
    await expect(body).toBeFocused();
  });

  test('the variable picker closes on Escape and shows the empty filter message', async ({ page }) => {
    await openSample(page);
    await page.getByRole('button', { name: 'Insert variable' }).first().click();
    await page.getByRole('combobox', { name: 'Filter variables' }).fill('zzz');
    await expect(page.getByText('No variables match the filter.')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('combobox', { name: 'Filter variables' })).toHaveCount(0);
  });

  test('rows can be added, moved with the keyboard and removed', async ({ page }) => {
    await openSample(page);
    const first = page.getByRole('combobox', { name: 'Row 1: Name', exact: true });

    await first.focus();
    await page.keyboard.press('Alt+Shift+ArrowDown');
    await expect(page.getByRole('combobox', { name: 'Row 2: Name', exact: true })).toHaveValue('x-order-id');
    await expect(page.getByText('Row moved to position 2 of 3')).toBeAttached();

    await page.getByRole('button', { name: 'Add row' }).click();
    await expect(page.getByRole('combobox', { name: 'Row 4: Name', exact: true })).toBeFocused();
    await page.getByRole('button', { name: 'Add row' }).click();
    await expect(page.getByRole('button', { name: 'Add row' })).toBeDisabled();
    await expect(page.getByText('Row limit reached. 5 of 5 rows')).toBeVisible();

    await page.getByRole('button', { name: 'Remove row 5' }).click();
    await expect(page.getByRole('button', { name: 'Add row' })).toBeEnabled();
    await expect(page.getByText('Header names cannot contain spaces.')).toBeVisible();
  });

  test('chips add on Enter, reject an invalid range with a message and remove with the button', async ({ page }) => {
    await openSample(page);
    const input = page.getByRole('combobox', { name: 'Allowed senders', exact: true });

    await input.fill('198.51.100.0/24');
    await input.press('Enter');
    await expect(page.getByRole('button', { name: 'Remove 198.51.100.0/24' })).toBeVisible();

    await input.fill('nonsense');
    await input.press('Enter');
    await expect(page.getByRole('alert').filter({ hasText: 'nonsense is not an address range' })).toBeVisible();
    await expect(input).toHaveValue('nonsense');

    await page.getByRole('button', { name: 'Remove 198.51.100.0/24' }).click();
    await expect(page.getByRole('button', { name: 'Remove 198.51.100.0/24' })).toHaveCount(0);
  });

  test('the capture picker pins the latest capture and follows again', async ({ page }) => {
    await openSample(page);
    await expect(page.getByText('Following the latest capture: POST /stripe')).toBeVisible();
    await page.getByRole('button', { name: 'Pin', exact: true }).first().click();
    await expect(page.getByText('Pinned to one capture')).toBeVisible();
    await page.getByRole('button', { name: 'Follow latest' }).click();
    await expect(page.getByText('Following the latest capture: POST /stripe')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Send a test request to this endpoint' })).toBeVisible();
  });
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the editors kit has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await openSample(page);
    await chooseTheme(page, theme);
    await expect(page.getByRole('textbox', { name: 'Row 1: Value', exact: true })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the open variable picker has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await openSample(page);
    await chooseTheme(page, theme);
    await page.getByRole('button', { name: 'Insert variable' }).first().click();
    await expect(page.getByRole('combobox', { name: 'Filter variables' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}

test('the editors kit fits a 360 px viewport without horizontal scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await openSample(page);
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});
