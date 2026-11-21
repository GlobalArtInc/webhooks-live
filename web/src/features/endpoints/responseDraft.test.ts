import { describe, expect, it } from 'vitest';
import {
  BODY_MAX_BYTES,
  buildEndpointInput,
  CONTENT_TYPE_MAX_BYTES,
  HEADER_NAME_MAX_BYTES,
  HEADER_VALUE_MAX_BYTES,
  createDefaultDraft,
  createHeaderRow,
  issuesFromServer,
  validateDraft,
  type HeaderRow,
  type ResponseDraft,
} from './responseDraft';

function draftWith(overrides: Partial<ResponseDraft>): ResponseDraft {
  return { ...createDefaultDraft(), ...overrides };
}

function headerRow(name: string, value = 'v'): HeaderRow {
  return { ...createHeaderRow(), name, value };
}

describe('response draft validation', () => {
  it('accepts the default response', () => {
    expect(validateDraft(createDefaultDraft()).size).toBe(0);
  });

  it('flags a status outside 100 to 599 and an empty status as required', () => {
    expect(validateDraft(draftWith({ status: '99' })).get('response.status')).toEqual({
      code: 'outOfRange',
      limit: null,
    });
    expect(validateDraft(draftWith({ status: '' })).get('response.status')?.code).toBe('required');
  });

  it('flags a body over 64 KiB by its UTF-8 size', () => {
    const issues = validateDraft(draftWith({ body: 'é'.repeat(BODY_MAX_BYTES / 2 + 1) }));
    expect(issues.get('response.body')).toEqual({ code: 'tooLong', limit: BODY_MAX_BYTES });
  });

  it('flags a duplicate header name case-insensitively on the later row', () => {
    const issues = validateDraft(draftWith({ headers: [headerRow('X-Trace'), headerRow('x-trace')] }));
    expect(issues.has('response.headers.0.name')).toBe(false);
    expect(issues.get('response.headers.1.name')?.code).toBe('duplicate');
  });

  it('flags a header name that is not an RFC 9110 token and a value with non-visible characters', () => {
    const issues = validateDraft(draftWith({ headers: [headerRow('Bad Name'), headerRow('X-Ok', 'café')] }));
    expect(issues.get('response.headers.0.name')?.code).toBe('invalid');
    expect(issues.get('response.headers.1.value')?.code).toBe('invalid');
  });

  it('builds the request from trimmed values with the default body encoding', () => {
    const input = buildEndpointInput(
      draftWith({
        status: ' 202 ',
        contentType: ' application/json ',
        body: '{"ok":true}',
        delay: '500',
        cors: true,
        headers: [headerRow(' X-Reply ', ' yes ')],
      }),
    );

    expect(input).toEqual({
      cors: true,
      response: {
        status: 202,
        content_type: 'application/json',
        body: { encoding: 'utf8', data: '{"ok":true}' },
        delay_ms: 500,
        headers: [{ name: 'X-Reply', value: 'yes' }],
      },
    });
  });

  it('maps server field codes onto specific issues with the documented limit', () => {
    const issues = issuesFromServer([
      { field: 'response.content_type', code: 'too_long', message: 'too long' },
      { field: 'response.headers.1.name', code: 'duplicate', message: 'duplicate' },
      { field: 'response.delay_ms', code: 'out_of_range', message: 'out of range' },
      { field: 'response.headers.0.value', code: 'not_allowed', message: 'denied' },
    ]);

    expect(issues.get('response.content_type')).toEqual({ code: 'tooLong', limit: CONTENT_TYPE_MAX_BYTES });
    expect(issues.get('response.headers.1.name')).toEqual({ code: 'duplicate', limit: HEADER_NAME_MAX_BYTES });
    expect(issues.get('response.delay_ms')).toEqual({ code: 'outOfRange', limit: null });
    expect(issues.get('response.headers.0.value')).toEqual({ code: 'notAllowed', limit: HEADER_VALUE_MAX_BYTES });
  });

  it('maps server field paths onto the same keys, folding body sub-fields into the body', () => {
    const issues = issuesFromServer([
      { field: 'response.status', message: 'must be between 100 and 599' },
      { field: 'response.body.data', message: 'too long' },
      { field: 'response.headers.2.name', message: 'not allowed' },
    ]);

    expect([...issues.keys()]).toEqual(['response.status', 'response.body', 'response.headers.2.name']);
    expect(issues.get('response.status')).toEqual({ code: 'invalid', limit: null });
  });
});
