import { bytesToBase64 } from './body';
import type { CaptureBody, CaptureExportInput } from './types';

export function utf8Body(data: string): CaptureBody {
  return { encoding: 'utf8', data, truncated: false };
}

export function binaryBody(bytes: readonly number[]): CaptureBody {
  return { encoding: 'base64', data: bytesToBase64(Uint8Array.from(bytes)), truncated: false };
}

export function makeCapture(overrides: Partial<CaptureExportInput> = {}): CaptureExportInput {
  return {
    id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b',
    seq: 7,
    endpoint_id: '7k2m9q4xw8c1d5f3h6j0n2p4',
    received_at: '2026-11-21T10:15:30.123Z',
    method: 'POST',
    path: '/orders',
    query_string: 'x=1&x=2',
    client_ip: '203.0.113.9',
    content_type: 'application/json',
    body_size: 0,
    status_override: null,
    response_status: 200,
    forward_state: null,
    url: 'https://hooks.example/7k2m9q4xw8c1d5f3h6j0n2p4/orders?x=1&x=2',
    host: 'hooks.example',
    headers: [],
    query: [],
    form: null,
    body: utf8Body(''),
    forwards: [],
    ...overrides,
  };
}
