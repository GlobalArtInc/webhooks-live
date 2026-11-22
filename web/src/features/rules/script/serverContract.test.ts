import { describe, expect, it } from 'vitest';
import { openApiFixture } from '../../../test/fixtures/openapi';
import { missingFieldsOf, schemaProperties, unknownFieldsOf } from '../../../test/openapiShapes';
import { fixtureValidate } from './fixtures';
import { SCRIPT_META_ANSWER } from './metaFixture';
import { parseScriptDiagnostics, parseScriptMeta } from './parse';
import { SCRIPT_META_PATH, SCRIPT_VALIDATE_PATH } from './constants';
import { httpScriptTransport } from './transport';

type Json = Record<string, unknown>;

function isJson(value: unknown): value is Json {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function field(value: unknown, key: string): unknown {
  return isJson(value) ? value[key] : undefined;
}

function listOf(value: unknown, key: string): Json[] {
  const found = field(value, key);
  return Array.isArray(found) ? found.filter(isJson) : [];
}

function record(value: unknown): Json {
  if (!isJson(value)) {
    throw new Error('expected an object');
  }
  return value;
}

describe('the script routes the server documents', () => {
  it('serves the catalogue by GET and the check by POST at the paths the transport calls', () => {
    const paths = field(openApiFixture, 'paths');
    expect(isJson(field(field(paths, SCRIPT_META_PATH), 'get'))).toBe(true);
    expect(isJson(field(field(paths, SCRIPT_VALIDATE_PATH), 'post'))).toBe(true);
  });

  it('answers the catalogue with the fields the interface parses and nothing it cannot place', () => {
    const answer = record(SCRIPT_META_ANSWER);
    expect(unknownFieldsOf('ScriptMeta', answer)).toEqual([]);
    expect(missingFieldsOf('ScriptMeta', answer)).toEqual([]);
    for (const item of listOf(answer, 'functions')) {
      expect(unknownFieldsOf('ScriptFunction', item)).toEqual([]);
      expect(missingFieldsOf('ScriptFunction', item)).toEqual([]);
    }
    for (const item of listOf(answer, 'variables')) {
      expect(unknownFieldsOf('ScriptVariable', item)).toEqual([]);
      expect(missingFieldsOf('ScriptVariable', item)).toEqual([]);
    }
    const limits = record(field(answer, 'limits'));
    expect(unknownFieldsOf('ScriptLimits', limits)).toEqual([]);
    expect(missingFieldsOf('ScriptLimits', limits)).toEqual([]);
    expect(unknownFieldsOf('ScriptPhaseLimits', record(field(limits, 'before_response')))).toEqual([]);
    expect(missingFieldsOf('ScriptPhaseLimits', record(field(limits, 'after_response')))).toEqual([]);

    const meta = parseScriptMeta(answer);

    expect(meta.name).toBe('rhai');
    expect(meta.functions.map((item) => item.name)).toContain('regex_match');
    expect(meta.functions.find((item) => item.name === 'regex_match')?.signature).toBe(
      'regex_match(pattern: string, text: string) -> bool',
    );
    expect(meta.variables.map((item) => item.name)).toContain('request.headers');
    expect(meta.limits.max_source_bytes).toBe(16384);
  });

  it('answers a check with the diagnostic fields the interface reads', () => {
    const answer = {
      diagnostics: [{ severity: 'error', code: 'syntax', message: "Unexpected ';'", line: 2, column: 9, length: 1 }],
    };
    expect(unknownFieldsOf('ScriptValidation', answer)).toEqual([]);
    const first = record(listOf(answer, 'diagnostics')[0]);
    expect(unknownFieldsOf('ScriptDiagnostic', first)).toEqual([]);
    expect(missingFieldsOf('ScriptDiagnostic', first)).toEqual([]);

    expect(parseScriptDiagnostics(answer)).toEqual([
      { severity: 'error', code: 'syntax', message: "Unexpected ';'", line: 2, column: 9, length: 1 },
    ]);
  });

  it('produces mock diagnostics that fit the documented schema', () => {
    const diagnostics = fixtureValidate('let a = eval("1");\nlet b = a +');
    expect(diagnostics.length).toBeGreaterThan(0);
    for (const item of diagnostics) {
      expect(unknownFieldsOf('ScriptDiagnostic', { ...item })).toEqual([]);
      expect(missingFieldsOf('ScriptDiagnostic', { ...item })).toEqual([]);
    }
  });

  it('sends only the fields the check accepts', async () => {
    const sent: unknown[] = [];
    const original = globalThis.fetch;
    globalThis.fetch = (_url, init) => {
      sent.push(JSON.parse(String(init?.body)));
      return Promise.resolve(Response.json({ diagnostics: [] }));
    };
    try {
      await httpScriptTransport.validate('let a = 1;', new AbortController().signal);
    } finally {
      globalThis.fetch = original;
    }

    expect(sent).toEqual([{ source: 'let a = 1;' }]);
    expect(unknownFieldsOf('ScriptValidateInput', record(sent[0]))).toEqual([]);
    expect(schemaProperties('ScriptValidateInput')).toContain('source');
  });
});
