import type { Page, Route } from '@playwright/test';
import type { Capture, CaptureSummary } from '../../src/api/types';
import { summaryOf } from '../../src/test/msw/store';
import { apiError } from './mockApi';

const DEFAULT_LIMIT = 50;
const CAPTURE_BODY_TEXT = '{"order":42}';

export interface CaptureApiState {
  captures: Capture[];
  deleted: string[];
}

interface ListPage {
  items: CaptureSummary[];
  next_cursor: string | null;
  total: number;
}

function pageOf(state: CaptureApiState, url: URL): ListPage {
  const limit = Number(url.searchParams.get('limit') ?? DEFAULT_LIMIT);
  const offset = Number(url.searchParams.get('cursor') ?? 0);
  const ordered = [...state.captures].sort((left, right) => right.seq - left.seq);
  const end = offset + limit;
  return {
    items: ordered.slice(offset, end).map(summaryOf),
    next_cursor: end < ordered.length ? String(end) : null,
    total: ordered.length,
  };
}

function captureIdOf(pathname: string, endpointId: string): string | null {
  const prefix = `/api/v1/endpoints/${endpointId}/captures/`;
  if (!pathname.startsWith(prefix)) {
    return null;
  }
  return pathname.slice(prefix.length).replace(/\/body$/, '');
}

async function answer(route: Route, state: CaptureApiState, endpointId: string): Promise<void> {
  const request = route.request();
  const url = new URL(request.url());
  const listPath = `/api/v1/endpoints/${endpointId}/captures`;
  if (url.pathname === listPath && request.method() === 'GET') {
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(pageOf(state, url)) });
    return;
  }
  if (url.pathname === listPath && request.method() === 'DELETE') {
    state.deleted.push('*');
    state.captures = [];
    await route.fulfill({ status: 204 });
    return;
  }
  const captureId = captureIdOf(url.pathname, endpointId);
  const capture = state.captures.find((item) => item.id === captureId);
  if (captureId === null) {
    await route.fulfill({ status: 404, contentType: 'application/json', body: '{}' });
    return;
  }
  if (url.pathname.endsWith('/body')) {
    await route.fulfill({ status: 200, contentType: 'application/json', body: CAPTURE_BODY_TEXT });
    return;
  }
  if (request.method() === 'DELETE') {
    state.deleted.push(captureId);
    state.captures = state.captures.filter((item) => item.id !== captureId);
    await route.fulfill({ status: 204 });
    return;
  }
  if (capture === undefined) {
    const missing = apiError(404, 'capture_not_found', 'Capture not found.');
    await route.fulfill({
      status: missing.status,
      contentType: 'application/json',
      body: JSON.stringify(missing.body),
    });
    return;
  }
  await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(capture) });
}

export async function mockCaptureApi(page: Page, endpointId: string, captures: Capture[]): Promise<CaptureApiState> {
  const state: CaptureApiState = { captures: [...captures], deleted: [] };
  await page.route(
    (url) => url.pathname.startsWith(`/api/v1/endpoints/${endpointId}/captures`),
    (route) => answer(route, state, endpointId),
  );
  return state;
}
