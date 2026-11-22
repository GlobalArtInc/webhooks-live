import { describe, expect, it } from 'vitest';
import {
  findHoverTarget,
  formatDurationMs,
  limitsFor,
  scriptCompletionEntries,
  scriptCompletionSource,
} from './catalogue';
import { fixtureValidate } from './fixtures';
import { SCRIPT_META_FIXTURE } from './metaFixture';

const QUERY = { prefix: '', insideTemplate: false, explicit: true };

describe('completion entries', () => {
  it('lists the catalogue functions with signature, doc and a call opener', async () => {
    const entries = await scriptCompletionSource(SCRIPT_META_FIXTURE)(QUERY);
    const sha = entries.find((entry) => entry.label === 'sha256');
    expect(sha).toMatchObject({
      kind: 'function',
      insertText: 'sha256(',
      detail: 'sha256(text: string) -> string',
    });
    expect(sha?.description).toContain('SHA-256');
  });

  it('adds variable paths and keywords after the functions', () => {
    const entries = scriptCompletionEntries(SCRIPT_META_FIXTURE);
    expect(entries.find((entry) => entry.label === 'request.body')).toMatchObject({
      kind: 'variable',
      detail: 'string',
    });
    expect(entries.find((entry) => entry.label === 'let')).toMatchObject({ kind: 'keyword' });
    expect(entries.length).toBe(
      SCRIPT_META_FIXTURE.functions.length + SCRIPT_META_FIXTURE.variables.length + SCRIPT_META_FIXTURE.keywords.length,
    );
  });
});

describe('findHoverTarget', () => {
  const line = 'let digest = sha256(request.body.length);';

  it('documents a called function over exactly its name', () => {
    const target = findHoverTarget(line, line.indexOf('sha256') + 2, SCRIPT_META_FIXTURE);
    expect(target?.entry).toMatchObject({ kind: 'function', title: 'sha256' });
    expect(line.slice(target?.from, target?.to)).toBe('sha256');
  });

  it('documents the hovered segment of a dotted variable and not the segment after it', () => {
    const request = findHoverTarget(line, line.indexOf('request') + 1, SCRIPT_META_FIXTURE);
    expect(request?.entry.title).toBe('request');
    const body = findHoverTarget(line, line.indexOf('body') + 1, SCRIPT_META_FIXTURE);
    expect(body?.entry).toMatchObject({ title: 'request.body', detail: 'string' });
    expect(line.slice(body?.from, body?.to)).toBe('body');
  });

  it('shows nothing for unknown names, keywords, plain words and the gaps between words', () => {
    expect(findHoverTarget(line, line.indexOf('length') + 1, SCRIPT_META_FIXTURE)).toBeNull();
    expect(findHoverTarget(line, 1, SCRIPT_META_FIXTURE)).toBeNull();
    expect(findHoverTarget(line, line.indexOf('digest') + 1, SCRIPT_META_FIXTURE)).toBeNull();
    expect(findHoverTarget(line, line.indexOf(' = ') + 1, SCRIPT_META_FIXTURE)).toBeNull();
  });

  it('does not document a function name that is not called', () => {
    expect(findHoverTarget('let sha256 = 1;', 6, SCRIPT_META_FIXTURE)).toBeNull();
  });

  it('shows nothing inside a string or after a line comment', () => {
    const inString = 'let note = "sha256(x)";';
    expect(findHoverTarget(inString, inString.indexOf('sha256') + 1, SCRIPT_META_FIXTURE)).toBeNull();
    const inComment = 'let a = 1; // sha256(x)';
    expect(findHoverTarget(inComment, inComment.indexOf('sha256') + 1, SCRIPT_META_FIXTURE)).toBeNull();
    const afterString = 'let note = "a"; sha256(x)';
    expect(findHoverTarget(afterString, afterString.indexOf('sha256') + 1, SCRIPT_META_FIXTURE)).not.toBeNull();
  });
});

describe('limits and fixtures', () => {
  it('picks the limits of the phase and formats durations', () => {
    expect(limitsFor(SCRIPT_META_FIXTURE, 'before').max_time_ms).toBe(100);
    expect(limitsFor(SCRIPT_META_FIXTURE, 'after').max_time_ms).toBe(1000);
    expect(formatDurationMs(100, 'en')).toBe('100 ms');
    expect(formatDurationMs(1000, 'en')).toBe('1 sec');
    expect(formatDurationMs(1500, 'en')).toBe('1,500 ms');
  });

  it('marks the line of a trailing operator and a disabled symbol', () => {
    expect(fixtureValidate('let a = 1;\nlet b = a +')).toEqual([
      expect.objectContaining({ line: 2, column: 11, code: 'syntax', severity: 'error' }),
    ]);
    expect(fixtureValidate('let a = eval("1");')[0]).toMatchObject({
      line: 1,
      column: 9,
      code: 'disabled_feature',
    });
    expect(fixtureValidate('let a = 1;')).toEqual([]);
  });
});
