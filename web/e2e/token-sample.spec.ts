import { expect, test } from '@playwright/test';

test('switches the theme on the token sample page and captures both themes', async ({ page }) => {
  await page.goto('/dev/tokens');

  await expect(page.getByRole('heading', { level: 1, name: 'Design tokens' })).toBeVisible();

  const html = page.locator('html');
  const backgroundOfBody = () => page.locator('body').evaluate((element) => getComputedStyle(element).backgroundColor);

  await page.getByRole('radio', { name: 'Light', exact: true }).check();
  await expect(html).toHaveAttribute('data-theme', 'light');
  const lightBackground = await backgroundOfBody();
  await page.screenshot({ path: 'e2e/screenshots/light.png', fullPage: true, animations: 'disabled' });

  await page.getByRole('radio', { name: 'Dark', exact: true }).check();
  await expect(html).toHaveAttribute('data-theme', 'dark');
  await expect.poll(backgroundOfBody).not.toBe(lightBackground);
  await page.screenshot({ path: 'e2e/screenshots/dark.png', fullPage: true, animations: 'disabled' });
});
