import type { Capture } from '../../api/types';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { PUBLIC_URL } from '../../test/fixtures/instance';

const HOST = 'webhooks.example.test';
const LEFT_BODY = '{"amount":2000,"currency":"eur","items":[{"sku":"a1","qty":1}]}';
const RIGHT_BODY = '{"items":[{"sku":"a1","qty":1}],"currency":"eur","amount":2500}';

export const LEFT_CAPTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a10';
export const RIGHT_CAPTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a11';
export const LARGE_CAPTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a12';

function byteLength(text: string): number {
  return new TextEncoder().encode(text).byteLength;
}

export const leftCaptureFixture: Capture = {
  id: LEFT_CAPTURE_ID,
  seq: 10,
  endpoint_id: ENDPOINT_ID,
  received_at: '2026-11-21T14:02:11.000Z',
  method: 'POST',
  path: '/stripe',
  query_string: 'mode=test',
  client_ip: '203.0.113.7',
  content_type: 'application/json',
  body_size: byteLength(LEFT_BODY),
  status_override: null,
  response_status: 202,
  forward_state: null,
  url: `${PUBLIC_URL}/${ENDPOINT_ID}/stripe?mode=test`,
  host: HOST,
  headers: [
    { name: 'Host', value: HOST },
    { name: 'Date', value: 'Sat, 21 Nov 2026 14:02:11 GMT' },
    { name: 'X-Request-Id', value: 'req-left' },
    { name: 'Content-Type', value: 'application/json' },
    { name: 'Stripe-Signature', value: 't=1732,v1=9a' },
  ],
  query: [{ name: 'mode', value: 'test' }],
  form: null,
  body: { encoding: 'utf8', data: LEFT_BODY, truncated: false },
  forwards: [],
};

export const rightCaptureFixture: Capture = {
  ...leftCaptureFixture,
  id: RIGHT_CAPTURE_ID,
  seq: 9,
  received_at: '2026-11-21T13:59:40.000Z',
  query_string: '',
  url: `${PUBLIC_URL}/${ENDPOINT_ID}/stripe`,
  body_size: byteLength(RIGHT_BODY),
  headers: [
    { name: 'Host', value: HOST },
    { name: 'Date', value: 'Sat, 21 Nov 2026 13:59:40 GMT' },
    { name: 'X-Request-Id', value: 'req-right' },
    { name: 'Content-Type', value: 'application/json' },
    { name: 'Stripe-Signature', value: 't=1731,v1=41' },
    { name: 'X-Retry', value: '1' },
    { name: 'X-Note', value: '<img src=x onerror="alert(1)">' },
  ],
  query: [],
  body: { encoding: 'utf8', data: RIGHT_BODY, truncated: false },
};

export const largeCaptureFixture: Capture = {
  ...leftCaptureFixture,
  id: LARGE_CAPTURE_ID,
  body_size: 2 * 1024 * 1024,
  body: { encoding: 'utf8', data: LEFT_BODY, truncated: true },
};
