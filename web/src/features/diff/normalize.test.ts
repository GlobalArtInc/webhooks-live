import { describe, expect, it } from 'vitest';
import { binaryCaptureFixture } from '../../test/fixtures/captures';
import { DEFAULT_NORMALISE_OPTIONS, isVolatileHeader, normaliseCapture } from './normalize';
import { leftCaptureFixture, rightCaptureFixture } from './testFixtures';

const RAW_BODY_OPTIONS = { ...DEFAULT_NORMALISE_OPTIONS, sortJsonKeys: false };

describe('normaliseCapture', () => {
  it('hides volatile headers by default and keeps the rest in original order', () => {
    const lines = normaliseCapture(rightCaptureFixture, DEFAULT_NORMALISE_OPTIONS).headers.split('\n');

    expect(lines).toEqual([
      'Host: webhooks.example.test',
      'Content-Type: application/json',
      'Stripe-Signature: t=1731,v1=41',
      'X-Retry: 1',
      'X-Note: <img src=x onerror="alert(1)">',
    ]);
  });

  it('shows volatile headers when hideVolatile is off', () => {
    const lines = normaliseCapture(rightCaptureFixture, { ...DEFAULT_NORMALISE_OPTIONS, hideVolatile: false }).headers;

    expect(lines).toContain('Date: Sat, 21 Nov 2026 13:59:40 GMT');
    expect(lines).toContain('X-Request-Id: req-right');
  });

  it('sorts header lines by name when sortHeaders is on', () => {
    const lines = normaliseCapture(rightCaptureFixture, {
      ...DEFAULT_NORMALISE_OPTIONS,
      sortHeaders: true,
    }).headers.split('\n');

    expect(lines.map((line) => line.split(':')[0])).toEqual([
      'Content-Type',
      'Host',
      'Stripe-Signature',
      'X-Note',
      'X-Retry',
    ]);
  });

  it.each([
    'Date',
    'x-forwarded-for',
    'CF-Ray',
    'traceparent',
    'Connection',
    'content-length',
    'Via',
    'X-Amzn-Trace-Id',
  ])('treats %s as a volatile header', (name) => {
    expect(isVolatileHeader(name)).toBe(true);
  });

  it.each(['User-Agent', 'Host', 'Accept'])('keeps %s, which is not volatile by default', (name) => {
    expect(isVolatileHeader(name)).toBe(false);
  });

  it('pretty prints JSON with keys sorted by default', () => {
    const body = normaliseCapture(rightCaptureFixture, DEFAULT_NORMALISE_OPTIONS).body;

    expect(body).toBe(
      [
        '{',
        '  "amount": 2500,',
        '  "currency": "eur",',
        '  "items": [',
        '    {',
        '      "qty": 1,',
        '      "sku": "a1"',
        '    }',
        '  ]',
        '}',
      ].join('\n'),
    );
  });

  it('keeps the received key order when sortJsonKeys is off', () => {
    const body = normaliseCapture(rightCaptureFixture, RAW_BODY_OPTIONS).body;

    expect(body.indexOf('"items"')).toBeLessThan(body.indexOf('"amount"'));
  });

  it('keeps a body that does not parse as JSON unchanged', () => {
    const capture = {
      ...rightCaptureFixture,
      body: { encoding: 'utf8' as const, data: '{"amount":', truncated: false },
    };

    expect(normaliseCapture(capture, DEFAULT_NORMALISE_OPTIONS).body).toBe('{"amount":');
  });

  it('shows a binary body as its size and the digest when one is known', () => {
    expect(normaliseCapture(binaryCaptureFixture, { ...DEFAULT_NORMALISE_OPTIONS, binaryDigest: 'abc123' }).body).toBe(
      'size: 4 bytes\nsha256: abc123',
    );
    expect(normaliseCapture(binaryCaptureFixture, DEFAULT_NORMALISE_OPTIONS).body).toBe('size: 4 bytes');
  });

  it('keeps query pairs in received order and sorts them when asked', () => {
    const capture = {
      ...leftCaptureFixture,
      query: [
        { name: 'b', value: '2' },
        { name: 'a', value: '1' },
      ],
    };

    expect(normaliseCapture(capture, DEFAULT_NORMALISE_OPTIONS).summary.query).toBe('b=2&a=1');
    expect(normaliseCapture(capture, { ...DEFAULT_NORMALISE_OPTIONS, sortQuery: true }).summary.query).toBe('a=1&b=2');
  });
});
