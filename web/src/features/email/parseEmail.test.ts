import { describe, expect, it } from 'vitest';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { MAX_FIELD_CHARS, MAX_LIST_ITEMS, MAX_PARTS } from './constants';
import { findHeader, readEmailView } from './parseEmail';
import { hostileEmailCapture, withKind, withMeta, withRawMeta } from './testFixtures';

describe('readEmailView', () => {
  it('ignores captures that are not email', () => {
    expect(readEmailView(jsonCaptureFixture)).toBeNull();
    expect(readEmailView(withKind('dns'))).toBeNull();
  });

  it('reads the documented meta of an email capture', () => {
    const view = readEmailView(hostileEmailCapture);
    expect(view?.meta.tls).toBe(true);
    expect(view?.meta.envelope_to).toEqual(['k7qm2xta9vnc@in.hooks.example']);
    expect(view?.meta.parts).toHaveLength(9);
    expect(view?.meta.parse_error).toBeNull();
    expect(view?.meta.parts_truncated).toBe(false);
  });

  it('accepts a single string where a list is documented and reports parse errors', () => {
    const view = readEmailView(
      withMeta({ from: 'a@example.test', parse_error: 'bad boundary', parts_truncated: true }),
    );
    expect(view?.meta.from).toEqual(['a@example.test']);
    expect(view?.meta.parse_error).toEqual({ detail: 'bad boundary' });
    expect(view?.meta.parts_truncated).toBe(true);
    expect(readEmailView(withMeta({ parse_error: true }))?.meta.parse_error).toEqual({ detail: null });
  });

  it('survives malformed meta by dropping what it cannot trust', () => {
    const view = readEmailView(
      withMeta({
        subject: 42,
        to: [1, null, 'ok@example.test', ''],
        parts: [
          { index: -1, size: 1 },
          { index: 1.5 },
          'text',
          { index: 2, content_type: 7, size: -4, disposition: 'weird', is_text: 'yes' },
          { index: 2, content_type: 'text/plain' },
        ],
      }),
    );
    expect(view?.meta.subject).toBeNull();
    expect(view?.meta.to).toEqual(['ok@example.test']);
    expect(view?.meta.parts).toEqual([
      {
        index: 2,
        content_type: 'application/octet-stream',
        filename: null,
        size: 0,
        disposition: null,
        is_text: false,
      },
    ]);
  });

  it('treats a missing meta as empty instead of failing', () => {
    const view = readEmailView(withRawMeta(null));
    expect(view?.meta.parts).toEqual([]);
    expect(view?.meta.subject).toBeNull();
  });

  it('caps the number of parts, list items and the length of strings', () => {
    const parts = Array.from({ length: MAX_PARTS + 50 }, (_unused, index) => ({ index }));
    const view = readEmailView(
      withMeta({
        parts,
        to: Array.from({ length: MAX_LIST_ITEMS + 20 }, (_unused, index) => `a${index}@example.test`),
        subject: 's'.repeat(MAX_FIELD_CHARS * 3),
      }),
    );
    expect(view?.meta.parts).toHaveLength(MAX_PARTS);
    expect(view?.meta.to).toHaveLength(MAX_LIST_ITEMS);
    expect(view?.meta.subject?.length).toBe(MAX_FIELD_CHARS + 1);
  });

  it('stops scanning a list after a bounded number of entries', () => {
    const noise = Array.from({ length: MAX_LIST_ITEMS * 10 }, () => '');
    const view = readEmailView(withMeta({ to: [...noise, 'late@example.test'] }));
    expect(view?.meta.to).toEqual([]);
  });
});

describe('findHeader', () => {
  it('finds a header case-insensitively and returns the first one', () => {
    const headers = [
      { name: 'Reply-To', value: 'first' },
      { name: 'reply-to', value: 'second' },
    ];
    expect(findHeader(headers, 'REPLY-TO')).toBe('first');
    expect(findHeader(headers, 'cc')).toBeNull();
  });
});
