import { http, type HttpHandler } from 'msw';
import type { Usage } from '../../api/usage';
import { usageFixture } from '../fixtures/usage';
import { jsonResponse } from './responses';

export interface UsageFailure {
  status: number;
  body: unknown;
  headers?: Record<string, string>;
}

export interface UsageStore {
  usage: Usage;
  failure: UsageFailure | null;
  requests: number;
}

const USAGE = '/api/v1/usage';

export const usageStore: UsageStore = { usage: usageFixture(), failure: null, requests: 0 };

export function resetUsageStore(): void {
  usageStore.usage = usageFixture();
  usageStore.failure = null;
  usageStore.requests = 0;
}

export const usageHandlers: HttpHandler[] = [
  http.get(USAGE, () => {
    usageStore.requests += 1;
    if (usageStore.failure !== null) {
      return jsonResponse(usageStore.failure.body, usageStore.failure.status, usageStore.failure.headers);
    }
    return jsonResponse(usageStore.usage);
  }),
];
