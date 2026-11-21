import type { SendResult, SentRequest } from '../../api/types';
import { ENDPOINT_ID } from './endpoint';

export const SEND_RESULT_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5d01';
export const SENT_REQUEST_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5d02';
export const SENT_TARGET = 'https://erp.example.test/orders/77/paid';

export const sendResultFixture: SendResult = {
  id: SEND_RESULT_ID,
  status: 200,
  headers: [
    { name: 'content-type', value: 'application/json' },
    { name: 'content-length', value: '22' },
  ],
  body: { encoding: 'utf8', data: '{"ok":true,"order":"77"}', truncated: false },
  duration_ms: 143,
  final_url: null,
  error: null,
  capture_id: null,
};

export const sentRequestFixture: SentRequest = {
  id: SENT_REQUEST_ID,
  status: 201,
  headers: [{ name: 'content-type', value: 'application/json' }],
  body: { encoding: 'utf8', data: '{"created":true}', truncated: false },
  duration_ms: 96,
  final_url: null,
  error: null,
  capture_id: null,
  request: {
    method: 'POST',
    url: SENT_TARGET,
    headers: [
      { name: 'content-type', value: 'application/json' },
      { name: 'authorization', value: '[redacted]' },
    ],
    body: { encoding: 'utf8', data: '{"event":"paid"}', truncated: false },
    follow_redirects: false,
    endpoint_id: ENDPOINT_ID,
  },
  created_at: '2026-11-21T10:20:00.000Z',
};
