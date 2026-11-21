import type { Page, Route } from '@playwright/test';
import type { Capture, CaptureSummary } from '../../src/api/types';
import { summaryOf } from '../../src/test/msw/store';
import { apiError } from './mockApi';

const DEFAULT_LIMIT = 50;
const CAPTURE_BODY_TEXT = '{"order":42}';

export interface CaptureApiState {
  captures: Capture[];
  deleted: string[];
  listRequests: string[];
}

interface ListPage {
  items: CaptureSummary[];
  next_cursor: string | null;
  total: number;
}

function matchesListFilters(capture: Capture, url: URL): boolean {
  const methods = url.searchParams.get('method')?.split(',') ?? [];
  if (methods.length > 0 && !methods.includes(capture.method.toUpperCase())) {
    return false;
  }
  const text = url.searchParams.get('q');
  return text === null || capture.path.toLowerCase().includes(text.toLowerCase());
}

function pageOf(state: CaptureApiState, url: URL, endpointId: string): ListPage {
  const limit = Number(url.searchParams.get('limit') ?? DEFAULT_LIMIT);
  const ascending = url.searchParams.get('order') === 'asc';
  const afterSeq = Number(url.searchParams.get('after_seq') ?? 0);
  const cursor = url.searchParams.get('cursor');
  const direction = ascending ? 1 : -1;
  const matching = state.captures.filter((capture) => matchesListFilters(capture, url));
  const ordered = [...matching]
    .filter((capture) => capture.seq > afterSeq)
    .sort((left, right) => direction * (left.seq - right.seq));
  const rest = cursor === null ? ordered : ordered.filter((capture) => direction * (capture.seq - Number(cursor)) > 0);
  const items = rest.slice(0, limit);
  return {
    items: items.map((capture) => ({ ...summaryOf(capture), endpoint_id: endpointId })),
    next_cursor: rest.length > limit ? String(items.at(-1)?.seq ?? 0) : null,
    total: matching.length,
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
    state.listRequests.push(url.search);
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(pageOf(state, url, endpointId)),
    });
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
  const state: CaptureApiState = { captures: [...captures], deleted: [], listRequests: [] };
  await page.route(
    (url) => url.pathname.startsWith(`/api/v1/endpoints/${endpointId}/captures`),
    (route) => answer(route, state, endpointId),
  );
  return state;
}
