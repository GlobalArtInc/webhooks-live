import { http, type HttpHandler } from 'msw';
import type { CaptureSummary, Endpoint } from '../../api/types';
import { errorResponse, jsonResponse } from './responses';

const INBOX_CAPTURES = '/api/v1/inbox/captures';
const ENDPOINT = '/api/v1/endpoints/:endpointId';
const FOLDERS = '/api/v1/folders';

export interface InboxFixtures {
  captures: CaptureSummary[];
  endpoints: Endpoint[];
  listStatus?: number;
}

function selectedCaptures(captures: CaptureSummary[], url: URL): CaptureSummary[] {
  const endpoints = url.searchParams.get('endpoints');
  if (endpoints === null) {
    return captures;
  }
  const ids = endpoints.split(',');
  return captures.filter((capture) => ids.includes(capture.endpoint_id));
}

export function inboxHandlers({ captures, endpoints, listStatus = 200 }: InboxFixtures): HttpHandler[] {
  return [
    http.get(INBOX_CAPTURES, ({ request }) => {
      if (listStatus !== 200) {
        return errorResponse(listStatus, 'endpoint_gone', 'One of the endpoints is gone');
      }
      const items = selectedCaptures(captures, new URL(request.url));
      return jsonResponse({ items, next_cursor: null });
    }),
    http.get(FOLDERS, () => jsonResponse({ items: [] })),
    http.get<{ endpointId: string }>(ENDPOINT, ({ params }) => {
      const endpoint = endpoints.find((entry) => entry.id === params.endpointId);
      return endpoint === undefined
        ? errorResponse(404, 'endpoint_not_found', 'Endpoint not found')
        : jsonResponse(endpoint);
    }),
  ];
}
