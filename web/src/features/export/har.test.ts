import { describe, expect, it } from 'vitest';
import { HAR_CREATOR_NAME, HAR_CREATOR_VERSION } from './constants';
import { toHar } from './har';
import { binaryBody, makeCapture, utf8Body } from './testFixtures';

const HEADERS = [
  { name: 'X-Tag', value: 'one' },
  { name: 'content-TYPE', value: 'application/json; charset=utf-8' },
  { name: 'X-Tag', value: 'two' },
];

describe('toHar', () => {
  it('builds a HAR 1.2 log with one entry and the required fields', () => {
    const capture = makeCapture({
      headers: HEADERS,
      query: [
        { name: 'x', value: '1' },
        { name: 'x', value: '2' },
      ],
      body: utf8Body('{"a":1}'),
      response_status: 202,
    });
    const { log } = toHar(capture);
    expect(log.version).toBe('1.2');
    expect(log.creator).toEqual({ name: HAR_CREATOR_NAME, version: HAR_CREATOR_VERSION });
    expect(log.entries).toHaveLength(1);
    const entry = log.entries[0];
    expect(entry?.startedDateTime).toBe(capture.received_at);
    expect(entry?.request.method).toBe('POST');
    expect(entry?.request.url).toBe(capture.url);
    expect(entry?.request.httpVersion).toBe('HTTP/1.1');
    expect(entry?.request.headers).toEqual(HEADERS);
    expect(entry?.request.queryString).toEqual(capture.query);
    expect(entry?.request.postData).toEqual({ mimeType: 'application/json; charset=utf-8', text: '{"a":1}' });
    expect(entry?.request.headersSize).toEqual(expect.any(Number));
    expect(entry?.request.bodySize).toBe(7);
    expect(entry?.response.status).toBe(202);
    expect(entry?.cache).toEqual({});
    expect(entry?.timings).toEqual({ send: 0, wait: 0, receive: 0 });
  });

  it('falls back to application/octet-stream without a content-type header', () => {
    const { log } = toHar(makeCapture({ body: utf8Body('abc'), content_type: 'text/plain' }));
    expect(log.entries[0]?.request.postData?.mimeType).toBe('application/octet-stream');
  });

  it('carries a binary body as base64 with an encoding marker', () => {
    const { log } = toHar(makeCapture({ body: binaryBody([0, 255, 1]) }));
    expect(log.entries[0]?.request.postData).toEqual({
      mimeType: 'application/octet-stream',
      text: 'AP8B',
      _encoding: 'base64',
    });
    expect(log.entries[0]?.request.bodySize).toBe(3);
  });

  it('omits postData for an empty body', () => {
    const { log } = toHar(makeCapture());
    expect(log.entries[0]?.request.postData).toBeUndefined();
    expect(log.entries[0]?.request.bodySize).toBe(0);
  });

  it('builds the response from the answered status and the configured response', () => {
    const { log } = toHar(makeCapture({ response_status: 404 }), {
      endpointResponse: {
        status: 200,
        content_type: 'application/json',
        headers: [{ name: 'X-Reply', value: 'yes' }],
        body: utf8Body('{"ok":true}'),
      },
    });
    const response = log.entries[0]?.response;
    expect(response?.status).toBe(404);
    expect(response?.headers).toEqual([
      { name: 'Content-Type', value: 'application/json' },
      { name: 'X-Reply', value: 'yes' },
    ]);
    expect(response?.content).toEqual({ size: 11, mimeType: 'application/json', text: '{"ok":true}' });
    expect(response?.bodySize).toBe(11);
  });
});
