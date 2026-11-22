import { http, type HttpHandler } from 'msw';
import { openApiFixture } from '../fixtures/openapi';
import { jsonResponse } from './responses';

export interface OpenApiFailure {
  status: number;
  body: unknown;
}

export interface OpenApiStore {
  document: unknown;
  failure: OpenApiFailure | null;
  requests: number;
}

const OPENAPI = '/api/v1/openapi.json';

export const openApiStore: OpenApiStore = { document: openApiFixture, failure: null, requests: 0 };

export function resetOpenApiStore(): void {
  openApiStore.document = openApiFixture;
  openApiStore.failure = null;
  openApiStore.requests = 0;
}

export const openApiHandlers: HttpHandler[] = [
  http.get(OPENAPI, () => {
    openApiStore.requests += 1;
    if (openApiStore.failure !== null) {
      return jsonResponse(openApiStore.failure.body, openApiStore.failure.status);
    }
    return jsonResponse(openApiStore.document);
  }),
];
