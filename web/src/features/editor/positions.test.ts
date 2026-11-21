import { describe, expect, it } from 'vitest';
import { insideTemplate } from './completion';
import { byteLength, diagnosticRange, positionToOffset } from './positions';
import type { EditorDiagnostic } from './types';

const TEXT = 'first\nsecond line\nthird';

function diagnostic(line: number, column: number, length: number): EditorDiagnostic {
  return { line, column, length, code: 'x', message: 'm' };
}

describe('positionToOffset', () => {
  it('maps one-based line and column to an offset', () => {
    expect(positionToOffset(TEXT, 1, 1)).toBe(0);
    expect(positionToOffset(TEXT, 2, 3)).toBe(8);
    expect(positionToOffset(TEXT, 3, 1)).toBe(18);
  });

  it('clamps a column past the end of its line to the line end', () => {
    expect(positionToOffset(TEXT, 1, 99)).toBe(5);
  });

  it('clamps a line past the end of the text to the last line', () => {
    expect(positionToOffset(TEXT, 9, 2)).toBe(19);
  });

  it('treats non-finite and non-positive positions as the first line and column', () => {
    expect(positionToOffset(TEXT, Number.NaN, 0)).toBe(0);
    expect(positionToOffset(TEXT, -4, -4)).toBe(0);
  });
});

describe('diagnosticRange', () => {
  it('covers the reported length', () => {
    expect(diagnosticRange(TEXT, diagnostic(2, 1, 6))).toEqual({ from: 6, to: 12 });
  });

  it('widens a zero length to one character', () => {
    expect(diagnosticRange(TEXT, diagnostic(2, 2, 0))).toEqual({ from: 7, to: 8 });
  });

  it('marks the last character of a line when the position is at its end', () => {
    expect(diagnosticRange(TEXT, diagnostic(1, 6, 1))).toEqual({ from: 4, to: 5 });
  });

  it('never leaves the document for a position past the end', () => {
    const range = diagnosticRange(TEXT, diagnostic(40, 40, 10));
    expect(range.to).toBeLessThanOrEqual(TEXT.length);
    expect(range.from).toBeLessThan(range.to);
  });

  it('returns an empty range for an empty document', () => {
    expect(diagnosticRange('', diagnostic(1, 1, 3))).toEqual({ from: 0, to: 0 });
  });
});

describe('byteLength', () => {
  it('counts UTF-8 bytes rather than characters', () => {
    expect(byteLength('abc')).toBe(3);
    expect(byteLength('é')).toBe(2);
  });
});

describe('insideTemplate', () => {
  const delimiters = { open: '{{', close: '}}' };

  it('is true after an opener without a closer', () => {
    expect(insideTemplate('a {{ request.', delimiters)).toBe(true);
  });

  it('is false after the closer or without delimiters', () => {
    expect(insideTemplate('a {{ x }} b', delimiters)).toBe(false);
    expect(insideTemplate('a {{ x', null)).toBe(false);
    expect(insideTemplate('plain', delimiters)).toBe(false);
  });
});
