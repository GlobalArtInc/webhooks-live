import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import { TESTER_ENDPOINT_ID, TESTER_NEW_CAPTURE_ID } from '../src/features/rules/tester/fixtures';
import { SLOW_MARKER, mockTesterApi, type TesterApiState } from './support/testerApi';

const BODY_FIELD = 'Response body of the Respond step';

async function openSample(page: Page): Promise<TesterApiState> {
  const api = await mockTesterApi(page);
  await page.goto('/dev/tester');
  await expect(page.getByRole('heading', { level: 1, name: 'Rule test runner' })).toBeVisible();
  await expect(page.getByRole('list', { name: 'Trace of the test run' })).toBeVisible();
  return api;
}

function tester(page: Page): Locator {
  return page.getByRole('region', { name: 'Test', exact: true });
}

function entry(page: Page, id: string): Locator {
  return page.getByTestId(`trace-${id}`);
}

function card(page: Page, name: string): Locator {
  return page.getByRole('list', { name: 'Builder cards' }).getByText(name, { exact: true });
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test.describe('rule test runner', () => {
  test('runs the draft by itself and stepping moves the selection and highlights the builder card', async ({
    page,
  }) => {
    await openSample(page);
    await expect(page.getByTestId('tester-selection')).toHaveText('Selected: conditions');
    await expect(card(page, 'Conditions')).toHaveAttribute('data-highlighted', 'true');

    await tester(page).getByRole('button', { name: 'Step', exact: true }).click();
    await expect(page.getByTestId('tester-selection')).toHaveText('Selected: step 1');
    await expect(card(page, 'Step 1: extract')).toHaveAttribute('data-highlighted', 'true');
    await expect(card(page, 'Conditions')).toHaveAttribute('data-highlighted', 'false');
    await expect(entry(page, 'step-0-1')).toContainText('not reached yet');

    await page.keyboard.press(']');
    await expect(page.getByTestId('tester-selection')).toHaveText('Selected: step 2');
    await expect(card(page, 'Step 2: respond')).toHaveAttribute('data-highlighted', 'true');
    await expect(entry(page, 'step-0-1')).toHaveAttribute('aria-current', 'step');

    await page.keyboard.press('[');
    await expect(page.getByTestId('tester-selection')).toHaveText('Selected: step 1');

    await tester(page).getByRole('button', { name: 'Reset' }).click();
    await expect(page.getByTestId('tester-selection')).toHaveText('Selected: conditions');
  });

  test('shows the conditions, the variable that a step produced and the simulated step', async ({ page }) => {
    await openSample(page);
    await expect(entry(page, 'rule-0')).toContainText('2 of 2 conditions true');
    await expect(entry(page, 'step-0-3')).toContainText('simulated');

    await entry(page, 'step-0-0').getByRole('button').click();
    await expect(entry(page, 'step-0-0')).toContainText('var.amount = 2000');
    const variables = tester(page).getByRole('heading', { name: 'Variables at entry 2' });
    await expect(variables).toBeVisible();
    await expect(tester(page).getByText('var.amount').last()).toBeVisible();
  });

  test('marks the result out of date after an edit and a late response never replaces a newer one', async ({
    page,
  }) => {
    const api = await openSample(page);
    const field = page.getByRole('textbox', { name: BODY_FIELD });
    const before = api.dryRuns.length;

    await field.fill(SLOW_MARKER);
    await expect(tester(page).getByText('Out of date')).toBeVisible();
    await expect.poll(() => api.dryRuns.length).toBeGreaterThan(before);

    await field.fill('fast');
    await expect(tester(page).getByText('Out of date')).toBeHidden();
    await tester(page)
      .getByRole('region', { name: 'Response', exact: true })
      .getByRole('button', { name: 'View' })
      .click();
    await expect(tester(page).getByLabel('Response body')).toHaveText('fast');

    await expect.poll(() => api.released).toContain(SLOW_MARKER);
    await expect(tester(page).getByLabel('Response body')).toHaveText('fast');
    await expect(tester(page).getByText('Out of date')).toBeHidden();
    expect(api.dryRuns.every((run) => !run.execute_http)).toBe(true);
  });

  test('Test all rules shows where a stop cut the chain and which rule answered', async ({ page }) => {
    const api = await openSample(page);

    await tester(page).getByRole('radio', { name: 'All rules' }).check();

    const rules = tester(page).getByRole('list', { name: 'Rules in run order' });
    await expect(rules.getByText('Stop here: later rules are skipped')).toBeVisible();
    await expect(rules.getByText('not run')).toBeVisible();
    await expect(tester(page).getByText(/Sender gets 401/)).toBeVisible();
    await expect(tester(page).getByText(/\(rule 2\)/)).toBeVisible();
    await expect.poll(() => api.dryRuns.at(-1)?.rules.length).toBe(3);

    await rules.getByRole('button', { name: 'Open the trace of Reject unsigned calls' }).click();
    await expect(entry(page, 'rule-1')).toHaveAttribute('aria-current', 'step');
  });

  test('Run for real lists the side effects, asks first and links the capture it created', async ({ page }) => {
    const api = await openSample(page);

    await tester(page).getByRole('button', { name: 'Run for real...' }).click();
    const dialog = page.getByRole('dialog', { name: 'Run for real?' });
    await expect(dialog).toContainText('Http, Increment');
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toBeHidden();
    expect(api.sends).toHaveLength(0);

    await tester(page).getByRole('button', { name: 'Run for real...' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Run for real', exact: true }).click();

    const link = tester(page).getByRole('link', { name: 'Open the new capture (new tab)' });
    await expect(link).toHaveAttribute('href', `/e/${TESTER_ENDPOINT_ID}/c/${TESTER_NEW_CAPTURE_ID}`);
    await expect(link).toHaveAttribute('target', '_blank');
    expect(api.sends).toHaveLength(1);
    expect(api.sends[0]?.url).toBe(`https://hooks.example.test/${TESTER_ENDPOINT_ID}/stripe?source=demo`);
    expect(api.sends[0]?.endpoint_id).toBe(TESTER_ENDPOINT_ID);
  });

  test('a viewer and a draft with errors cannot run', async ({ page }) => {
    const api = await openSample(page);
    const runs = api.dryRuns.length;

    await page.getByRole('checkbox', { name: 'Draft has errors' }).check();
    await expect(tester(page).getByRole('button', { name: 'Run', exact: true })).toBeDisabled();
    await page.getByRole('textbox', { name: BODY_FIELD }).fill('changed while blocked');
    await expect(tester(page).getByText('Out of date')).toBeVisible();
    await expect(tester(page).getByText('Waiting for you to finish editing...')).toBeHidden();
    await expect(tester(page).getByText(/Fix the errors in the draft/)).toBeVisible();
    expect(api.dryRuns.length).toBe(runs);

    await page.getByRole('checkbox', { name: 'Draft has errors' }).uncheck();
    await page.getByRole('checkbox', { name: 'Read-only (viewer)' }).check();
    await expect(tester(page).getByText('Only editors can run tests.')).toBeVisible();
    await expect(tester(page).getByRole('button', { name: 'Run for real...' })).toBeDisabled();
  });
});

for (const theme of ['Light', 'Dark'] as const) {
  test(`the test runner has no axe violations in the ${theme.toLowerCase()} theme`, async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await openSample(page);
    await chooseTheme(page, theme);
    await entry(page, 'step-0-1').getByRole('button').click();
    await tester(page)
      .getByRole('region', { name: 'Response', exact: true })
      .getByRole('button', { name: 'View' })
      .click();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  test(`the test all view and the real run dialog have no axe violations in the ${theme.toLowerCase()} theme`, async ({
    page,
  }) => {
    await openSample(page);
    await chooseTheme(page, theme);
    await tester(page).getByRole('radio', { name: 'All rules' }).check();
    await expect(tester(page).getByText('Stop here: later rules are skipped')).toBeVisible();

    const table = await new AxeBuilder({ page }).analyze();
    expect(table.violations).toEqual([]);

    await tester(page).getByRole('button', { name: 'Run for real...' }).click();
    await expect(page.getByRole('dialog', { name: 'Run for real?' })).toBeVisible();
    const dialog = await new AxeBuilder({ page }).analyze();
    expect(dialog.violations).toEqual([]);
  });
}

test('the test runner fits a 360 px viewport without horizontal scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await openSample(page);
  await entry(page, 'step-0-0').getByRole('button').click();

  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});
