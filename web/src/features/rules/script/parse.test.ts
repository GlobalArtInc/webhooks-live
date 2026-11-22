import { describe, expect, it } from 'vitest';
import { MAX_DIAGNOSTICS } from './constants';
import { SCRIPT_META_FIXTURE } from './metaFixture';
import { ScriptContractError, parseScriptDiagnostics, parseScriptMeta } from './parse';

function diagnostic(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return { line: 2, column: 3, length: 4, code: 'script_invalid', message: 'Bad', severity: 'error', ...overrides };
}

describe('parseScriptMeta', () => {
  it('accepts the documented shape and keeps it intact', () => {
    expect(parseScriptMeta(structuredClone(SCRIPT_META_FIXTURE))).toEqual(SCRIPT_META_FIXTURE);
  });

  it('accepts a missing reference address as none', () => {
    const payload: Record<string, unknown> = { ...SCRIPT_META_FIXTURE };
    delete payload.reference_url;
    expect(parseScriptMeta(payload).reference_url).toBeNull();
  });

  it('refuses a reference address that is not an absolute path or https', () => {
    for (const bad of ['javascript:alert(1)', '//evil.example', 'http://plain.example', 'data:text/html,x']) {
      expect(() => parseScriptMeta({ ...SCRIPT_META_FIXTURE, reference_url: bad })).toThrow(ScriptContractError);
    }
  });

  it('refuses wrong types, bad names and non-positive limits', () => {
    const limits = { ...SCRIPT_META_FIXTURE.limits, max_source_bytes: 0 };
    expect(() => parseScriptMeta({ ...SCRIPT_META_FIXTURE, limits })).toThrow(/max_source_bytes/);
    expect(() => parseScriptMeta({ ...SCRIPT_META_FIXTURE, keywords: ['ok', 3] })).toThrow(/keywords\[1\]/);
    expect(() =>
      parseScriptMeta({ ...SCRIPT_META_FIXTURE, functions: [{ name: 'a b', signature: '', doc: '' }] }),
    ).toThrow(/functions\[0\]\.name/);
    expect(() => parseScriptMeta(null)).toThrow(ScriptContractError);
  });
});

describe('parseScriptDiagnostics', () => {
  it('reads positions, code, message and severity', () => {
    expect(parseScriptDiagnostics({ diagnostics: [diagnostic()] })).toEqual([diagnostic()]);
  });

  it('treats a missing severity as an error and refuses an unknown one', () => {
    const [first] = parseScriptDiagnostics({ diagnostics: [diagnostic({ severity: undefined })] });
    expect(first?.severity).toBe('error');
    expect(() => parseScriptDiagnostics({ diagnostics: [diagnostic({ severity: 'fatal' })] })).toThrow(/severity/);
  });

  it('refuses positions that are zero, fractional or not numbers', () => {
    for (const bad of [0, 1.5, '3', null]) {
      expect(() => parseScriptDiagnostics({ diagnostics: [diagnostic({ line: bad })] })).toThrow(/line/);
    }
    expect(() => parseScriptDiagnostics({ diagnostics: 'none' })).toThrow(ScriptContractError);
  });

  it('keeps at most the first fifty problems', () => {
    const many = Array.from({ length: MAX_DIAGNOSTICS + 20 }, (_, index) => diagnostic({ line: index + 1 }));
    const parsed = parseScriptDiagnostics({ diagnostics: many });
    expect(parsed).toHaveLength(MAX_DIAGNOSTICS);
    expect(parsed[0]?.line).toBe(1);
  });
});
