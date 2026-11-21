import type { Capture, ForwardJob } from '../../api/types';
import { ENDPOINT_ID } from './endpoint';
import { PUBLIC_URL } from './instance';

const HOST = 'webhooks.example.test';

const JSON_BODY = '{"order":42}';
const FORM_BODY = [
  '--wl',
  'Content-Disposition: form-data; name="order"',
  '',
  '42',
  '--wl',
  'Content-Disposition: form-data; name="invoice"; filename="invoice.pdf"',
  'Content-Type: application/pdf',
  '',
  '%PDF-',
  '--wl--',
  '',
].join('\r\n');
const PAID_BODY = '{"event":"paid"}';

function byteLength(text: string): number {
  return new TextEncoder().encode(text).byteLength;
}

export const forwardedJobFixture: ForwardJob = {
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5b01',
  kind: 'rule',
  rule_id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5c01',
  target_url: 'https://api.example.test/hooks',
  method: 'POST',
  state: 'succeeded',
  attempts: [
    {
      attempt: 1,
      started_at: '2026-11-21T10:16:21.800Z',
      duration_ms: 84,
      status: 200,
      error: null,
      final_url: null,
    },
  ],
  created_at: '2026-11-21T10:16:21.790Z',
  finished_at: '2026-11-21T10:16:21.890Z',
};

export const jsonCaptureFixture: Capture = {
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a01',
  seq: 1,
  endpoint_id: ENDPOINT_ID,
  received_at: '2026-11-21T10:16:02.511Z',
  method: 'POST',
  path: '/404/orders',
  query_string: 'x=1',
  client_ip: '203.0.113.7',
  content_type: 'application/json',
  body_size: byteLength(JSON_BODY),
  status_override: 404,
  response_status: 404,
  forward_state: null,
  url: `${PUBLIC_URL}/${ENDPOINT_ID}/404/orders?x=1`,
  host: HOST,
  headers: [
    { name: 'Host', value: HOST },
    { name: 'Content-Type', value: 'application/json' },
  ],
  query: [{ name: 'x', value: '1' }],
  form: null,
  body: { encoding: 'utf8', data: JSON_BODY, truncated: false },
  forwards: [],
};

export const formCaptureFixture: Capture = {
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a02',
  seq: 2,
  endpoint_id: ENDPOINT_ID,
  received_at: '2026-11-21T10:16:09.120Z',
  method: 'POST',
  path: '/forms/upload',
  query_string: '',
  client_ip: '203.0.113.8',
  content_type: 'multipart/form-data; boundary=wl',
  body_size: byteLength(FORM_BODY),
  status_override: null,
  response_status: 200,
  forward_state: null,
  url: `${PUBLIC_URL}/${ENDPOINT_ID}/forms/upload`,
  host: HOST,
  headers: [
    { name: 'Host', value: HOST },
    { name: 'Content-Type', value: 'multipart/form-data; boundary=wl' },
  ],
  query: [],
  form: {
    fields: [{ name: 'order', value: '42' }],
    files: [{ name: 'invoice', filename: 'invoice.pdf', content_type: 'application/pdf', size: 5 }],
  },
  body: { encoding: 'utf8', data: FORM_BODY, truncated: false },
  forwards: [],
};

export const binaryCaptureFixture: Capture = {
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a03',
  seq: 3,
  endpoint_id: ENDPOINT_ID,
  received_at: '2026-11-21T10:16:15.004Z',
  method: 'PUT',
  path: '/blobs/logo.png',
  query_string: '',
  client_ip: '203.0.113.9',
  content_type: 'image/png',
  body_size: 4,
  status_override: null,
  response_status: 200,
  forward_state: null,
  url: `${PUBLIC_URL}/${ENDPOINT_ID}/blobs/logo.png`,
  host: HOST,
  headers: [
    { name: 'Host', value: HOST },
    { name: 'Content-Type', value: 'image/png' },
  ],
  query: [],
  form: null,
  body: { encoding: 'base64', data: 'iVBORw==', truncated: false },
  forwards: [],
};

export const forwardedCaptureFixture: Capture = {
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a04',
  seq: 4,
  endpoint_id: ENDPOINT_ID,
  received_at: '2026-11-21T10:16:21.777Z',
  method: 'POST',
  path: '/hooks/payment',
  query_string: '',
  client_ip: '203.0.113.10',
  content_type: 'application/json',
  body_size: byteLength(PAID_BODY),
  status_override: null,
  response_status: 200,
  forward_state: 'succeeded',
  url: `${PUBLIC_URL}/${ENDPOINT_ID}/hooks/payment`,
  host: HOST,
  headers: [
    { name: 'Host', value: HOST },
    { name: 'Content-Type', value: 'application/json' },
  ],
  query: [],
  form: null,
  body: { encoding: 'utf8', data: PAID_BODY, truncated: false },
  forwards: [forwardedJobFixture],
};

export const captureFixtures: Capture[] = [
  jsonCaptureFixture,
  formCaptureFixture,
  binaryCaptureFixture,
  forwardedCaptureFixture,
];
