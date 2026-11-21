import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';

async function openSample(page: Page): Promise<void> {
  await page.goto('/dev/conditions');
  await expect(page.getByRole('heading', { level: 1, name: 'Rule condition editor' })).toBeVisible();
  await expect(page.getByRole('group', { name: /^Condition 1(?!\d)/ })).toBeVisible();
}

function row(page: Page, number: number): Locator {
  return page.getByRole('group', { name: new RegExp(`^Condition ${number}(?!\\d)`) });
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

async function makeRegexRow(page: Page, pattern: string): Promise<void> {
  await row(page, 1).getByLabel('Operator').selectOption('matches');
  const editor = row(page, 1).getByRole('textbox', { name: 'Value' });
  await editor.click();
  await page.keyboard.press('ControlOrMeta+A');
  await page.keyboard.type(pattern);
}

test.describe('condition rows', () => {
  test('every row is a named group with labelled controls, and the value widget follows the operator', async ({
    page,
  }) => {
    await openSample(page);

    await expect(row(page, 1).getByLabel('Subject')).toHaveValue('method');
    await expect(row(page, 2).getByLabel('Header name')).toHaveValue('x-event-type');
    await expect(page.getByRole('button', { name: 'Remove condition 4' })).toBeVisible();
    await expect(row(page, 3).getByLabel('Value')).toBeVisible();

    await row(page, 2).getByLabel('Operator').selectOption('exists');
    await expect(row(page, 2).getByLabel('Value')).toHaveCount(0);
  });

  test('a new row is added from the keyboard, focuses its subject and stays marked until finished', async ({
    page,
  }) => {
    await openSample(page);
    const add = page.getByRole('button', { name: 'Add condition' }).last();

    await add.focus();
    await page.keyboard.press('Enter');

    await expect(row(page, 5).getByLabel('Subject')).toBeFocused();
    await expect(row(page, 5).getByText('Incomplete')).toBeVisible();
    await page.keyboard.press('ArrowDown');
    await expect(row(page, 5).getByLabel('Subject')).not.toHaveValue('');
  });

  test('removing a row moves focus to the Add button of its group and announces it', async ({ page }) => {
    await openSample(page);

    await page.getByRole('button', { name: 'Remove condition 2' }).click();

    await expect(page.getByText('Condition 2 removed')).toBeAttached();
    await expect(page.getByRole('button', { name: 'Add condition' }).last()).toBeFocused();
    await expect(row(page, 2).getByLabel('Subject')).toHaveValue('json');
  });

  test('chips take a list value with Enter and refuse a bad address range with a message', async ({ page }) => {
    await openSample(page);
    const sender = row(page, 4);
    const input = sender.getByRole('textbox', { name: 'Value' });

    await input.fill('300.1.1.1/8');
    await input.press('Enter');
    await expect(sender.getByRole('alert').filter({ hasText: '300.1.1.1/8 is not an address range.' })).toBeVisible();
    await expect(input).toHaveValue('300.1.1.1/8');

    await input.fill('198.51.100.0/24');
    await input.press('Enter');
    await expect(sender.getByRole('button', { name: 'Remove 198.51.100.0/24' })).toBeVisible();
  });
});

test.describe('save, field errors and conflicts', () => {
  test('an unfinished row blocks the save, is explained on the row and takes focus', async ({ page }) => {
    await openSample(page);

    await page.getByRole('button', { name: 'Add condition' }).last().click();
    await page.getByRole('button', { name: 'Save' }).click();

    await expect(row(page, 5).getByText('Choose a subject.')).toBeVisible();
    await expect(row(page, 5).getByLabel('Subject')).toBeFocused();
    await expect(page.getByText('2 problems in the conditions.')).toBeAttached();
    await expect(page.getByRole('heading', { name: 'Last request body' })).toHaveCount(0);
  });

  test('a server field error lands on its row, focuses its control and goes away when the row is edited', async ({
    page,
  }) => {
    await openSample(page);

    await makeRegexRow(page, '(');
    await page.getByRole('button', { name: 'Save' }).click();

    await expect(row(page, 1).getByText(/Invalid regular expression/)).toBeVisible();
    await expect(row(page, 1).getByRole('textbox', { name: 'Value' })).toBeFocused();

    await page.keyboard.type('x)');
    await expect(row(page, 1).getByText(/Invalid regular expression/)).toHaveCount(0);
  });

  test('a saved draft sends the documented condition shape and the bar goes clean', async ({ page }) => {
    await openSample(page);

    await row(page, 1).getByLabel('Value').fill('PUT');
    await page.getByRole('radio', { name: /Stop here/ }).check();
    await page.keyboard.press('ControlOrMeta+S');

    await expect(page.getByText('Conditions saved.')).toBeAttached();
    await expect(page.getByRole('button', { name: 'Save' })).toBeDisabled();
    const body = page.getByRole('region', { name: 'Last request body' });
    await expect(body).toContainText('"on_match": "stop"');
    await expect(body).toContainText('"arg": "PUT"');
    await expect(body).toContainText('"not"');
  });

  test('a change made elsewhere while editing shows the conflict banner and Reload takes the new rule', async ({
    page,
  }) => {
    await openSample(page);

    await row(page, 1).getByLabel('Value').fill('PUT');
    await page.getByRole('button', { name: 'Simulate a change made elsewhere' }).click();

    await expect(page.getByRole('alert').filter({ hasText: 'Changed elsewhere' })).toBeVisible();
    await page.getByRole('button', { name: 'Reload' }).click();
    await expect(page.getByText(/Changed elsewhere/)).toHaveCount(0);
    await expect(row(page, 1).getByLabel('Value')).toHaveValue('POST');
    await expect(page.getByRole('radio', { name: /Stop here/ })).toBeChecked();
  });

  test('Keep mine and overwrite saves the draft against the new version', async ({ page }) => {
    await openSample(page);

    await row(page, 1).getByLabel('Value').fill('PUT');
    await page.getByRole('button', { name: 'Simulate a change made elsewhere' }).click();
    await page.getByRole('button', { name: 'Keep mine and overwrite' }).click();
    await page.getByRole('button', { name: 'Save' }).click();

    await expect(page.getByText('Conditions saved.')).toBeAttached();
    await expect(page.getByText('Server version 9')).toBeVisible();
  });

  test('Discard asks first and then restores the saved conditions', async ({ page }) => {
    await openSample(page);

    await row(page, 1).getByLabel('Value').fill('PUT');
    await page.getByRole('button', { name: 'Discard' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Discard' }).click();

    await expect(row(page, 1).getByLabel('Value')).toHaveValue('POST');
  });

  test('a condition the editor cannot show is read only and never rewritten', async ({ page }) => {
    await openSample(page);

    await page.getByLabel('Load a condition the editor cannot show').check();

    await expect(page.getByRole('alert').filter({ hasText: 'read only' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Save' })).toHaveCount(0);
  });
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the editor has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await openSample(page);
    await chooseTheme(page, theme);

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the editor with problems shown has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await openSample(page);
    await chooseTheme(page, theme);
    await makeRegexRow(page, '(');
    await page.getByRole('button', { name: 'Add condition' }).last().click();
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(row(page, 5).getByText('Choose a subject.')).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}

test('the editor fits a 360 px viewport without horizontal scrolling and keeps 44 px targets', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await openSample(page);

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
  const box = await page.getByRole('button', { name: 'Remove condition 1' }).boundingBox();
  expect(box?.width ?? 0).toBeGreaterThanOrEqual(44);
  expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
});
