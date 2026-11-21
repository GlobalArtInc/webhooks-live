import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import { ENDPOINT_ID } from './support/mockApi';
import {
  EDITOR_RULE_ID,
  RECORDED_RUN_ID,
  SCRIPT_RULE_ID,
  mockRuleEditorApi,
  type RuleEditorApiState,
} from './support/ruleEditorApi';

const EDITOR_PATH = `/e/${ENDPOINT_ID}/settings/rules/${EDITOR_RULE_ID}`;

async function openEditor(page: Page, path: string = EDITOR_PATH, width = 1440): Promise<RuleEditorApiState> {
  await page.setViewportSize({ width, height: 900 });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const state = await mockRuleEditorApi(page);
  await page.goto(path);
  await expect(page.getByRole('textbox', { name: 'Rule name' })).toBeVisible();
  return state;
}

function nameField(page: Page): Locator {
  return page.getByRole('textbox', { name: 'Rule name' });
}

function conditionValue(page: Page): Locator {
  return page.getByRole('group', { name: /^Condition 1(?!\d)/ }).getByRole('textbox', { name: 'Value' });
}

function saveButton(page: Page): Locator {
  return page.getByRole('button', { name: 'Save', exact: true });
}

async function chooseTheme(page: Page, theme: 'Light' | 'Dark'): Promise<void> {
  await page.getByRole('radio', { name: theme, exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
}

test.describe('rule editor', () => {
  test('editing a condition enables Save and saving sends one PATCH with the edited rule', async ({ page }) => {
    const api = await openEditor(page);
    await expect(saveButton(page)).toBeDisabled();

    await conditionValue(page).click();
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.type('PUT');
    await expect(saveButton(page)).toBeEnabled();
    await saveButton(page).click();

    await expect.poll(() => api.patches.length).toBe(1);
    const [patch] = api.patches;
    expect(patch?.ruleId).toBe(EDITOR_RULE_ID);
    expect(Object.keys(patch?.body ?? {}).sort()).toEqual(['condition', 'name', 'steps']);
    expect(JSON.stringify(patch?.body.condition)).toContain('PUT');
    await expect(saveButton(page)).toBeDisabled();
  });

  test('a rule changed elsewhere shows the conflict banner instead of overwriting, and Reload takes theirs', async ({
    page,
  }) => {
    const api = await openEditor(page);

    api.rename(EDITOR_RULE_ID, 'Renamed by a teammate');
    await nameField(page).fill('Renamed by me');
    await saveButton(page).click();

    const banner = page.getByRole('alert').filter({ hasText: 'Changed elsewhere' });
    await expect(banner).toBeVisible();
    expect(api.patches).toHaveLength(0);

    await banner.getByRole('button', { name: 'Reload' }).click();
    await expect(nameField(page)).toHaveValue('Renamed by a teammate');
    await expect(banner).toBeHidden();
  });

  test('a server field error lands on its step field and focuses it', async ({ page }) => {
    const api = await openEditor(page);
    api.rejectNextPatch = [{ field: 'steps.1.status', code: 'invalid_format', message: 'is not a valid status' }];

    await nameField(page).fill('Mark paid orders v2');
    await saveButton(page).click();

    await expect(page.getByText('is not a valid status')).toBeVisible();
    await expect(page.getByRole('region', { name: 'Step 2 of 2: Respond' })).toHaveAttribute('data-invalid', 'true');
  });

  test('the tester runs the draft and stepping highlights the matching card', async ({ page }) => {
    const api = await openEditor(page);
    const trace = page.getByRole('list', { name: 'Trace of the test run' });
    await expect(trace).toBeVisible();
    await expect.poll(() => api.dryRuns.length).toBeGreaterThan(0);
    expect(api.dryRuns[0]?.rules).toHaveLength(1);

    await nameField(page).fill('Edited while testing');
    await expect.poll(() => api.dryRuns.at(-1)?.rules[0]?.name).toBe('Edited while testing');

    await page
      .getByRole('region', { name: 'Test', exact: true })
      .getByRole('button', { name: 'Step', exact: true })
      .click();
    await expect(page.getByRole('region', { name: 'Step 1 of 2: Extract value' })).toHaveAttribute(
      'data-highlighted',
      'true',
    );
  });

  test('a script rule shows one full-height script editor', async ({ page }) => {
    await openEditor(page, `/e/${ENDPOINT_ID}/settings/rules/${SCRIPT_RULE_ID}`);

    await expect(page.getByRole('group', { name: 'Script' })).toBeVisible();
    await expect(page.getByRole('list', { name: 'Steps', exact: true })).toHaveCount(0);
  });

  test('narrow screens use tabs driven by the tab parameter and the tester keeps the draft', async ({ page }) => {
    await openEditor(page, EDITOR_PATH, 360);
    await expect(page.getByRole('tab', { name: 'Build' })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('region', { name: 'Test', exact: true })).toBeHidden();

    await page.getByRole('tab', { name: 'Test' }).click();
    await expect(page).toHaveURL(/tab=test/);
    await expect(page.getByRole('region', { name: 'Test', exact: true })).toBeVisible();
    await expect(page.getByRole('group', { name: /^Condition 1(?!\d)/ })).toBeHidden();
  });

  test('Ctrl+Enter on a narrow screen opens the Test tab and runs the draft', async ({ page }) => {
    const api = await openEditor(page, EDITOR_PATH, 360);
    const before = api.dryRuns.length;

    await nameField(page).press('ControlOrMeta+Enter');

    await expect(page).toHaveURL(/tab=test/);
    await expect(page.getByRole('region', { name: 'Test', exact: true })).toBeVisible();
    await expect.poll(() => api.dryRuns.length).toBeGreaterThan(before);
  });

  test('leaving with unsaved changes asks first, and Keep editing stays on the rule', async ({ page }) => {
    await openEditor(page);
    await nameField(page).fill('Changed but not saved');

    await page.getByRole('link', { name: 'Back to rules' }).click();
    const dialog = page.getByRole('dialog', { name: 'Discard unsaved changes?' });
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Keep editing' }).click();

    await expect(page).toHaveURL(new RegExp(`/rules/${EDITOR_RULE_ID}`));
    await expect(nameField(page)).toHaveValue('Changed but not saved');
  });

  test('a tester variable is inserted into the template field that last had focus, and not before', async ({
    page,
  }) => {
    await openEditor(page);
    const tester = page.getByRole('region', { name: 'Test', exact: true });
    await expect(tester.getByRole('button', { name: 'Copy request.method' })).toBeVisible();
    await expect(tester.getByRole('button', { name: 'Insert request.method' })).toHaveCount(0);

    await page
      .getByRole('button', { name: /Respond/ })
      .first()
      .click();
    const body = page.getByRole('textbox', { name: 'Body' });
    await body.click();
    await expect(tester.getByRole('button', { name: 'Insert request.method' })).toBeVisible();
    await tester.getByRole('button', { name: 'Insert request.method' }).click();

    await expect(body).toContainText('{{ request.method }}');
    await expect(saveButton(page)).toBeEnabled();
  });

  test('Open in tester on a recorded run pins its capture and explains that the trace is a new test', async ({
    page,
  }) => {
    await openEditor(page, `${EDITOR_PATH}?tab=runs`);

    await page.getByRole('link', { name: 'Open in tester: Mark paid orders' }).click();

    await expect(page).toHaveURL(new RegExp(`tab=test.*run=${RECORDED_RUN_ID}|run=${RECORDED_RUN_ID}.*tab=test`));
    await expect(page).toHaveURL(/capture=/);
    await expect(page.getByText('This test runs your current draft on the same capture')).toBeVisible();
    await expect(page.getByRole('region', { name: 'Test', exact: true })).toBeVisible();
  });

  test('the build tab works from the keyboard alone', async ({ page }) => {
    const api = await openEditor(page);

    await nameField(page).focus();
    await page.keyboard.press('End');
    await page.keyboard.type(' v3');
    await page.keyboard.press('Tab');
    await expect(page.getByRole('switch', { name: 'Rule enabled' })).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(page.getByRole('tab', { name: 'Build' })).toBeFocused();

    const extract = page.getByRole('button', { name: /Extract value/ }).first();
    await extract.focus();
    await page.keyboard.press('Enter');
    await expect(extract).toHaveAttribute('aria-expanded', 'true');
    await page.keyboard.press('Escape');
    await expect(extract).toHaveAttribute('aria-expanded', 'false');

    await page.keyboard.press('ControlOrMeta+S');
    await expect.poll(() => api.patches.length).toBe(1);
    expect(api.patches[0]?.body.name).toBe('Mark paid orders v3');
  });

  test('has no axe violations on the test tab at 360 px', async ({ page }) => {
    await openEditor(page, `${EDITOR_PATH}?tab=test`, 360);
    await expect(page.getByRole('list', { name: 'Trace of the test run' })).toBeVisible();

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });

  for (const width of [360, 1440]) {
    for (const theme of ['Light', 'Dark'] as const) {
      test(`has no axe violations at ${width} px in the ${theme.toLowerCase()} theme`, async ({ page }) => {
        await openEditor(page, EDITOR_PATH, width);
        await chooseTheme(page, theme);
        await expect(page.getByRole('list', { name: 'Steps', exact: true })).toBeVisible();

        const results = await new AxeBuilder({ page }).analyze();
        expect(results.violations).toEqual([]);
      });
    }
  }
});
