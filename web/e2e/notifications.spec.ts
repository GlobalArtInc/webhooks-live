import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { endpointFixture } from '../src/test/fixtures/endpoint';
import { instanceFixture, PUBLIC_URL } from '../src/test/fixtures/instance';
import { emailChannelFixture, slackChannelFixture } from '../src/test/fixtures/notifications';
import { mockCaptureApi } from './support/captureApi';
import { installFakeStream } from './support/fakeStream';
import { ENDPOINT_ID, instanceWith, mockApi, type Reply, type ReplyMap } from './support/mockApi';

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/notifications`;
const TEST_PATH = `POST /endpoints/${ENDPOINT_ID}/notifications/${slackChannelFixture.id}/test`;
const CHANNELS_PATH = `GET /endpoints/${ENDPOINT_ID}/notifications`;
const NOTIFICATION_FEATURES = { ...instanceFixture.features, notifications: ['webhook', 'slack', 'telegram'] };
const DELIVERED_AT = '2026-11-21T10:30:00.000Z';

function delivery(state: 'succeeded' | 'failed'): Reply {
  return {
    status: 202,
    body: {
      id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4e01',
      channel_id: slackChannelFixture.id,
      kind: 'test',
      state,
      attempts: 1,
      status: state === 'failed' ? 401 : 200,
      error: state === 'failed' ? 'Unauthorized' : null,
      created_at: DELIVERED_AT,
      finished_at: DELIVERED_AT,
    },
  };
}

function sectionReplies(testReply: Reply): ReplyMap {
  return {
    'GET /instance': instanceWith({ features: NOTIFICATION_FEATURES }),
    [`GET /endpoints/${ENDPOINT_ID}`]: {
      body: { ...endpointFixture, id: ENDPOINT_ID, url: `${PUBLIC_URL}/${ENDPOINT_ID}`, my_role: 'owner' },
    },
    [CHANNELS_PATH]: { body: { items: [slackChannelFixture, emailChannelFixture] } },
    [TEST_PATH]: testReply,
  };
}

async function openSection(page: Page, testReply: Reply): Promise<void> {
  await page.setViewportSize({ width: 360, height: 740 });
  await mockApi(page, sectionReplies(testReply));
  await mockCaptureApi(page, ENDPOINT_ID, []);
  await installFakeStream(page);
  await page.goto(SECTION_PATH);
  await expect(page.getByRole('heading', { level: 2, name: 'Notifications' })).toBeVisible();
}

async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

for (const colorScheme of ['light', 'dark'] as const) {
  test(`the notification channels fit 360 px and have no serious accessibility issues in ${colorScheme} theme`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme });
    await openSection(page, delivery('succeeded'));

    await expect(page.getByRole('list', { name: 'Notification channels' })).toBeVisible();
    expect(await horizontalOverflow(page)).toBe(0);

    const results = await new AxeBuilder({ page }).analyze();
    const serious = results.violations.filter(
      (violation) => violation.impact === 'serious' || violation.impact === 'critical',
    );
    expect(serious).toEqual([]);
  });
}

test('a successful test send shows the result inline next to its channel', async ({ page }) => {
  await openSection(page, delivery('succeeded'));

  await page.getByRole('button', { name: 'Send a test to ops-chat' }).click();

  await expect(page.getByText(/^Test delivered at /)).toBeVisible();
});

test('a failed test send shows the status and the error inline', async ({ page }) => {
  await openSection(page, delivery('failed'));

  await page.getByRole('button', { name: 'Send a test to ops-chat' }).click();

  await expect(page.getByRole('alert').filter({ hasText: '401 Unauthorized' })).toBeVisible();
});
