import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';

const ORDER = 'Draft order: Extract value > Respond > Send request';

async function openSample(page: Page): Promise<void> {
  await page.goto('/dev/steps');
  await expect(page.getByRole('heading', { level: 1, name: 'Rule steps' })).toBeVisible();
  await expect(page.getByTestId('steps-order')).toHaveText(ORDER);
}

function draft(page: Page): Locator {
  return page.getByRole('region', { name: 'Rule draft', exact: true });
}

function kinds(page: Page): Locator {
  return page.getByRole('region', { name: 'Every step type', exact: true });
}

function card(scope: Locator, position: number, total: number, title: string): Locator {
  return scope.getByRole('region', { name: `Step ${position} of ${total}: ${title}`, exact: true });
}

function headingButton(scope: Locator): Locator {
  return scope.getByRole('heading', { level: 3 }).getByRole('button');
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test.describe('step cards', () => {
  test('collapsed cards show summary, guard line and phase badge, and expand from the heading', async ({ page }) => {
    await openSample(page);
    const extract = card(draft(page), 1, 3, 'Extract value');
    const respond = card(draft(page), 2, 3, 'Respond');
    const http = card(draft(page), 3, 3, 'Send request');

    await expect(extract.getByText('from request.body into amount')).toBeVisible();
    await expect(respond.getByText('Before response')).toBeVisible();
    await expect(http.getByText('After response')).toBeVisible();
    await expect(http.getByText('only if var.amount is greater than 0')).toBeVisible();

    const toggle = headingButton(extract);
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await expect(extract.getByText('Produces')).toBeVisible();
    await expect(extract.getByText('var.amount')).toBeVisible();
  });

  test('Esc collapses an expanded card and keeps focus on its heading', async ({ page }) => {
    await openSample(page);
    const extract = card(draft(page), 1, 3, 'Extract value');
    const toggle = headingButton(extract);

    await toggle.click();
    await extract.getByRole('textbox', { name: /^From/ }).focus();
    await page.keyboard.press('Escape');

    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(toggle).toBeFocused();
  });

  test('a server error lands on its field and the focus button reaches it', async ({ page }) => {
    await openSample(page);
    await draft(page).getByRole('checkbox', { name: 'Show server errors' }).check();
    const http = card(draft(page), 3, 3, 'Send request');

    await expect(http.getByText('1 error')).toBeVisible();
    await expect(
      card(draft(page), 2, 3, 'Respond').getByText('A response step cannot come after a step that sends a request.'),
    ).toBeVisible();

    await draft(page).getByRole('button', { name: 'Focus the URL field of the request step' }).click();
    await expect(http.getByText('The address host is not allowed on this plan.')).toBeVisible();
    await expect(http.getByRole('textbox', { name: 'URL', exact: true })).toBeFocused();
  });

  test('the guard editor enables a condition and an incomplete one is marked', async ({ page }) => {
    await openSample(page);
    const respond = card(draft(page), 2, 3, 'Respond');

    await headingButton(respond).click();
    await respond.getByRole('checkbox', { name: 'Run this step only if a value matches' }).check();
    await expect(respond.getByText('Fill in the value and what to compare it with.')).toBeVisible();

    await respond.getByRole('textbox', { name: 'Value', exact: true }).fill('var.amount');
    await respond.getByRole('combobox', { name: 'Operator' }).selectOption('exists');
    await expect(respond.getByText('Fill in the value and what to compare it with.')).toHaveCount(0);
    await headingButton(respond).click();
    await expect(respond.getByText('only if var.amount exists')).toBeVisible();
  });
});

test.describe('insertion and order', () => {
  test('a step added between two others lands in that position, expanded with its first field focused', async ({
    page,
  }) => {
    await openSample(page);

    await draft(page).getByRole('button', { name: 'Add step at position 2' }).click();
    await page.getByRole('combobox', { name: 'Filter step types' }).fill('delay');
    await page.keyboard.press('Enter');

    await expect(page.getByTestId('steps-order')).toHaveText(
      'Draft order: Extract value > Delay > Respond > Send request',
    );
    const delay = card(draft(page), 2, 4, 'Delay');
    await expect(headingButton(delay)).toHaveAttribute('aria-expanded', 'true');
    await expect(delay.getByRole('spinbutton', { name: /Delay/ })).toBeFocused();
    await expect(draft(page).getByText('Delay added at position 2 of 4')).toBeAttached();
  });

  test('the add menu groups step types by class and filters them', async ({ page }) => {
    await openSample(page);
    await draft(page).getByRole('button', { name: 'Add step at position 4' }).click();
    const list = page.getByRole('listbox', { name: 'Step types' });

    for (const group of ['Response', 'Values', 'Stored state', 'Fetch', 'Side effects']) {
      await expect(list.getByText(group, { exact: true })).toBeVisible();
    }
    await page.getByRole('combobox', { name: 'Filter step types' }).fill('zzz');
    await expect(page.getByText('No step type matches the filter.')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(list).toHaveCount(0);
  });

  test('buttons and Alt+Shift+Arrow move a step and announce the position', async ({ page }) => {
    await openSample(page);

    await draft(page).getByRole('button', { name: 'Move step 1 down' }).click();
    await expect(page.getByTestId('steps-order')).toHaveText('Draft order: Respond > Extract value > Send request');
    await expect(draft(page).getByText('Extract value moved to position 2 of 3')).toBeAttached();

    const toggle = headingButton(card(draft(page), 3, 3, 'Send request'));
    await toggle.focus();
    await page.keyboard.press('Alt+Shift+ArrowUp');
    await expect(page.getByTestId('steps-order')).toHaveText('Draft order: Respond > Send request > Extract value');
    await expect(draft(page).getByText('Send request moved to position 2 of 3')).toBeAttached();
    await expect(headingButton(card(draft(page), 2, 3, 'Send request'))).toBeFocused();
  });

  test('Delete on a heading removes the step and Undo puts it back in place', async ({ page }) => {
    await openSample(page);

    await headingButton(card(draft(page), 2, 3, 'Respond')).focus();
    await page.keyboard.press('Delete');
    await expect(page.getByTestId('steps-order')).toHaveText('Draft order: Extract value > Send request');

    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(page.getByTestId('steps-order')).toHaveText(ORDER);
  });

  test('the step limit disables every insertion point', async ({ page }) => {
    await openSample(page);
    for (let count = 3; count < 8; count += 1) {
      await draft(page)
        .getByRole('button', { name: `Add step at position ${count + 1}` })
        .click();
      await page.getByRole('combobox', { name: 'Filter step types' }).fill('stop');
      await page.keyboard.press('Enter');
      await expect(draft(page).getByText(`${count + 1} of 8 steps`)).toBeVisible();
    }

    await expect(draft(page).getByText('Step limit reached. 8 of 8 steps')).toBeVisible();
    const buttons = draft(page).getByRole('button', { name: /^Add step at position/ });
    for (let index = 0; index < 8; index += 1) {
      await expect(buttons.nth(index)).toBeDisabled();
    }
  });
});

test.describe('field kinds', () => {
  test('every field kind renders from the fixture schema', async ({ page }) => {
    await openSample(page);
    const scope = kinds(page);

    await headingButton(scope.getByRole('region', { name: /: Respond$/ })).click();
    const respond = scope.getByRole('region', { name: /: Respond$/ });
    await expect(respond.getByRole('textbox', { name: 'Status', exact: true })).toBeVisible();
    await expect(respond.getByRole('textbox', { name: 'Body', exact: true })).toBeVisible();
    await expect(respond.getByRole('button', { name: 'Add row' })).toBeVisible();

    await headingButton(scope.getByRole('region', { name: /: Delay$/ })).click();
    await expect(scope.getByRole('spinbutton', { name: /Delay/ })).toBeVisible();

    await headingButton(scope.getByRole('region', { name: /: Extract value$/ })).click();
    await expect(scope.getByRole('textbox', { name: /^JSON path/ })).toBeVisible();

    await headingButton(scope.getByRole('region', { name: /: Send request$/ })).click();
    await expect(scope.getByRole('combobox', { name: /^Method/ })).toBeVisible();
    await expect(scope.getByRole('checkbox', { name: 'Follow redirects' })).toBeChecked();

    await headingButton(scope.getByRole('region', { name: /: Set header$/ })).click();
    await expect(scope.getByRole('textbox', { name: /^Header name/ })).toBeVisible();

    await headingButton(scope.getByRole('region', { name: /: Script$/ })).click();
    await expect(scope.getByRole('textbox', { name: 'Script', exact: true })).toBeVisible();

    await headingButton(scope.getByRole('region', { name: /: Notify$/ })).click();
    await expect(scope.getByRole('combobox', { name: /^Channel/ })).toBeVisible();

    await headingButton(scope.getByRole('region', { name: /: Store in S3$/ })).click();
    const secret = scope.getByRole('combobox', { name: /^Access key secret/ });
    await expect(secret).toBeVisible();
    await expect(secret.getByRole('option')).toHaveText([
      'Choose a secret',
      'CRM_TOKEN',
      'ARCHIVE_ACCESS',
      'ARCHIVE_SIGNING',
    ]);
  });

  test('a template field inserts a variable at the cursor and keeps focus in the editor', async ({ page }) => {
    await openSample(page);
    const scope = kinds(page);
    const setVar = scope.getByRole('region', { name: /: Set variable$/ });

    await headingButton(setVar).click();
    const value = setVar.getByRole('textbox', { name: 'Value', exact: true });
    await value.click();
    await setVar.getByRole('button', { name: 'Insert variable into Value' }).click();
    await page.getByRole('combobox', { name: 'Filter variables' }).fill('amount');
    await page.keyboard.press('Enter');

    await expect(value).toContainText('{{ request.json.data.amount }}');
    await expect(value).toBeFocused();
  });

  test('a key-value header row takes a name and a templated value', async ({ page }) => {
    await openSample(page);
    const respond = card(draft(page), 2, 3, 'Respond');

    await headingButton(respond).click();
    await expect(respond.getByRole('textbox', { name: 'Row 1: Value', exact: true })).toContainText('{{ var.amount }}');
    await respond.getByRole('button', { name: 'Add row' }).click();
    const name = respond.getByRole('textbox', { name: 'Row 2: Header name', exact: true });
    await expect(name).toBeFocused();
    await name.fill('X Bad');
    await expect(respond.getByText(/Header names use letters/)).toBeVisible();
  });
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the steps page has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await openSample(page);
    await chooseTheme(page, theme);
    await draft(page).getByRole('checkbox', { name: 'Show server errors' }).check();
    await headingButton(card(draft(page), 2, 3, 'Respond')).click();
    await headingButton(card(draft(page), 3, 3, 'Send request')).click();
    await expect(
      card(draft(page), 3, 3, 'Send request').getByRole('textbox', { name: 'URL', exact: true }),
    ).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the open add-step menu has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await openSample(page);
    await chooseTheme(page, theme);
    await draft(page).getByRole('button', { name: 'Add step at position 2' }).click();
    await expect(page.getByRole('combobox', { name: 'Filter step types' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
}

test('the steps page fits a 360 px viewport without horizontal scrolling, with cards expanded', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await openSample(page);
  await headingButton(card(draft(page), 2, 3, 'Respond')).click();
  await headingButton(card(draft(page), 3, 3, 'Send request')).click();
  await expect(
    card(draft(page), 3, 3, 'Send request').getByRole('textbox', { name: 'URL', exact: true }),
  ).toBeVisible();

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
  const button = card(draft(page), 1, 3, 'Extract value').getByRole('button', { name: 'Move step 1 down' });
  const box = await button.boundingBox();
  expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
});
