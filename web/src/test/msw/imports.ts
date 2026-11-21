import { http, type HttpHandler } from 'msw';
import { ENDPOINT_ID } from '../fixtures/endpoint';
import { jsonResponse } from './responses';

type HeaderMap = Readonly<Record<string, string>>;

export interface ImportReply {
  status: number;
  body: unknown;
  headers?: HeaderMap;
}

export interface ImportRecord {
  path: string;
  keepTimes: string | null;
  contentType: string | null;
}

export interface ImportHandlerOptions {
  into?: ImportReply;
  created?: ImportReply;
  onRequest?: (record: ImportRecord) => void;
}

const IMPORT_INTO = '/api/v1/endpoints/:endpointId/import';
const IMPORT_NEW = '/api/v1/endpoints/import';
const NEW_ENDPOINT_LOCATION = `/api/v1/endpoints/${ENDPOINT_ID}`;

const DEFAULT_INTO: ImportReply = { status: 200, body: { imported: 2, skipped: 0, reason: null } };
const DEFAULT_CREATED: ImportReply = {
  status: 201,
  body: { endpoint_id: ENDPOINT_ID, imported: 2, skipped: 0, reason: null },
  headers: { Location: NEW_ENDPOINT_LOCATION },
};

function replyWith(reply: ImportReply): Response {
  return jsonResponse(reply.body, reply.status, reply.headers ?? {});
}

export function importHandlers(options: ImportHandlerOptions = {}): HttpHandler[] {
  const record = (request: Request): void => {
    const url = new URL(request.url);
    options.onRequest?.({
      path: url.pathname,
      keepTimes: url.searchParams.get('keep_times'),
      contentType: request.headers.get('Content-Type'),
    });
  };
  return [
    http.post(IMPORT_INTO, ({ request }) => {
      record(request);
      return replyWith(options.into ?? DEFAULT_INTO);
    }),
    http.post(IMPORT_NEW, ({ request }) => {
      record(request);
      return replyWith(options.created ?? DEFAULT_CREATED);
    }),
  ];
}
