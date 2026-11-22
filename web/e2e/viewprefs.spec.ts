import { expect, test } from '@playwright/test';
import { SCREENS } from './support/screens';

const DETAIL = SCREENS.find((screen) => screen.name === 'detail');

test('the body view mode and wrap survive a reload and a bad stored value falls back to the defaults', async ({
  page,
}) => {
  if (DETAIL === undefined) {
    throw new Error('the detail screen is not defined');
  }
  await DETAIL.open(page);
  const raw = page.getByRole('group', { name: 'Display mode' }).getByRole('button', { name: 'Raw', exact: true });
  const wrap = page
    .getByRole('toolbar', { name: 'Body viewer controls' })
    .getByRole('button', { name: 'Wrap', exact: true });
  await expect(raw).toHaveAttribute('aria-pressed', 'false');

  await raw.click();
  await wrap.click();
  await expect(raw).toHaveAttribute('aria-pressed', 'true');
  await expect(wrap).toHaveAttribute('aria-pressed', 'true');

  await page.reload();
  await expect(
    page.getByRole('group', { name: 'Display mode' }).getByRole('button', { name: 'Raw', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  await expect(
    page.getByRole('toolbar', { name: 'Body viewer controls' }).getByRole('button', { name: 'Wrap', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');

  await page.evaluate(() => {
    window.localStorage.setItem('webhooks-live:prefs:v1', '{"bodyMode":"hex","bodyWrap":"maybe","rail":"gone"}');
  });
  await page.reload();
  await expect(
    page.getByRole('group', { name: 'Display mode' }).getByRole('button', { name: 'Pretty', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  await expect(
    page.getByRole('toolbar', { name: 'Body viewer controls' }).getByRole('button', { name: 'Wrap', exact: true }),
  ).toHaveAttribute('aria-pressed', 'false');
});
