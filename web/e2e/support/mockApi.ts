import type { Page, Route } from '@playwright/test';
import type { Instance } from '../../src/api/types';
import { instanceFixture } from '../../src/test/fixtures/instance';

export const ENDPOINT_ID = '0123456789abcdefghjkmnpq';

export interface Reply {
  status?: number;
  body?: unknown;
}

export type ReplyMap = Record<string, Reply>;

export function apiError(status: number, code: string, message: string): Reply {
  return { status, body: { error: { code, message } } };
}

export async function mockApi(page: Page, replies: ReplyMap): Promise<void> {
  await page.route('**/api/v1/**', (route) => answer(route, replies));
  await page.route('**/auth/logout', (route) => route.fulfill({ status: 204 }));
}

export function instanceWith(overrides: Partial<Instance> = {}): Reply {
  return { body: { ...instanceFixture, ...overrides } };
}

async function answer(route: Route, replies: ReplyMap): Promise<void> {
  const request = route.request();
  const path = new URL(request.url()).pathname.replace('/api/v1', '');
  const reply = replies[`${request.method()} ${path}`];
  if (reply === undefined) {
    await route.fulfill({
      status: 404,
      contentType: 'application/json',
      body: JSON.stringify({ error: { code: 'not_mocked', message: path } }),
    });
    return;
  }
  await route.fulfill({
    status: reply.status ?? 200,
    contentType: 'application/json',
    body: JSON.stringify(reply.body ?? {}),
  });
}
