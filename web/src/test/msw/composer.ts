import { http, type HttpHandler } from 'msw';
import type { SendRequest, SentRequest } from '../../api/types';
import { sendResultFixture } from '../fixtures/composer';
import { emptyResponse, jsonResponse, readJsonBody } from './responses';

const SEND = '/api/v1/send';
const SEND_HISTORY = '/api/v1/send/history';
const SEND_HISTORY_ITEM = '/api/v1/send/history/:sentId';

export interface SendHandlerOptions {
  requests?: SendRequest[];
  history?: SentRequest[];
  reply?: (input: SendRequest) => Response;
}

export function sendHandlers(options: SendHandlerOptions = {}): HttpHandler[] {
  const { requests = [], history = [], reply = () => jsonResponse(sendResultFixture) } = options;
  return [
    http.post(SEND, async ({ request }) => {
      const input = await readJsonBody<SendRequest>(request);
      if (input !== null) {
        requests.push(input);
      }
      return reply(input ?? { method: 'GET', url: '', headers: [], body: null });
    }),
    http.get(SEND_HISTORY, () => jsonResponse({ items: history, next_cursor: null })),
    http.delete(SEND_HISTORY_ITEM, () => emptyResponse()),
  ];
}
