import { describe, expect, it } from 'vitest';
import { toJson } from './json';
import { binaryBody, makeCapture, utf8Body } from './testFixtures';

describe('toJson', () => {
  it('exports the whole capture in a stable shape with 2-space indentation', () => {
    const capture = makeCapture({
      headers: [
        { name: 'X-Tag', value: 'one' },
        { name: 'X-Tag', value: 'two' },
      ],
      query: [{ name: 'x', value: '1' }],
      body: utf8Body('hello'),
      body_size: 5,
    });
    const text = toJson(capture);
    expect(text.startsWith('{\n  "format": "webhooks.live/capture",\n  "version": 1,\n')).toBe(true);
    const parsed: unknown = JSON.parse(text);
    expect(parsed).toEqual({
      format: 'webhooks.live/capture',
      version: 1,
      id: capture.id,
      seq: 7,
      endpoint_id: capture.endpoint_id,
      received_at: capture.received_at,
      method: 'POST',
      url: capture.url,
      host: 'hooks.example',
      path: '/orders',
      query_string: 'x=1&x=2',
      client_ip: '203.0.113.9',
      content_type: 'application/json',
      status_override: null,
      response_status: 200,
      forward_state: null,
      headers: capture.headers,
      query: capture.query,
      form: null,
      body: { encoding: 'utf8', data: 'hello', truncated: false, size: 5 },
      forwards: [],
    });
  });

  it('keeps a binary body as base64 with its encoding marker', () => {
    const parsed: unknown = JSON.parse(toJson(makeCapture({ body: binaryBody([0, 255]), body_size: 2 })));
    expect(parsed).toMatchObject({ body: { encoding: 'base64', data: 'AP8=', size: 2 } });
  });
});
