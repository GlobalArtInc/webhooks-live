import { describe, expect, it } from 'vitest';
import type { ForwardRule } from '../../api/types';
import { draftFromRule, issueFromServer, ruleInputFromDraft, splitHeaderNames, validateTarget } from './ruleDraft';

const RULE: ForwardRule = {
  id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21',
  enabled: true,
  target_url: 'https://hooks.example.com/in',
  method: null,
  content_type: 'application/json',
  header_allowlist: ['x-sig', 'x-id'],
  append_path: true,
};

describe('validateTarget', () => {
  it('accepts absolute http and https URLs and rejects everything else', () => {
    expect(validateTarget('https://hooks.example.com/in')).toBeNull();
    expect(validateTarget('http://example.com')).toBeNull();
    expect(validateTarget('')).toEqual({ code: 'required', detail: null });
    expect(validateTarget('hooks.example.com/in')).toEqual({ code: 'invalid', detail: null });
    expect(validateTarget('ftp://example.com/file')).toEqual({ code: 'invalid', detail: null });
  });
});

describe('ruleInputFromDraft', () => {
  it('turns blank optional fields into null and splits the header names', () => {
    const input = ruleInputFromDraft(
      { targetUrl: ' https://a.example/x ', method: ' ', contentType: '', headers: 'x-a, ,x-b ', appendPath: true },
      false,
    );
    expect(input).toEqual({
      enabled: false,
      target_url: 'https://a.example/x',
      method: null,
      content_type: null,
      header_allowlist: ['x-a', 'x-b'],
      append_path: true,
    });
  });

  it('round-trips a stored rule through the draft', () => {
    const draft = draftFromRule(RULE);
    expect(draft.headers).toBe('x-sig, x-id');
    expect(ruleInputFromDraft(draft, RULE.enabled)).toMatchObject({
      target_url: RULE.target_url,
      content_type: 'application/json',
      header_allowlist: RULE.header_allowlist,
      append_path: true,
    });
  });
});

describe('splitHeaderNames', () => {
  it('keeps only non-empty trimmed names', () => {
    expect(splitHeaderNames(' a , , b,')).toEqual(['a', 'b']);
  });
});

describe('issueFromServer', () => {
  it('maps a refused target to its field code and keeps the server reason', () => {
    expect(
      issueFromServer([{ field: 'target_url', code: 'target_refused', message: '10.0.0.5 is a private address' }]),
    ).toEqual({ code: 'targetRefused', detail: '10.0.0.5 is a private address' });
  });

  it('ignores errors on other fields', () => {
    expect(issueFromServer([{ field: 'method', code: 'invalid_format', message: 'bad' }])).toBeNull();
  });
});
