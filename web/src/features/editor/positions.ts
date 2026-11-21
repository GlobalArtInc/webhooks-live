import type { EditorDiagnostic, TextRange } from './types';

const LINE_BREAK = '\n';
const MIN_LENGTH = 1;

function lineStartOffset(text: string, line: number): number {
  let offset = 0;
  for (let current = 1; current < line; current += 1) {
    const next = text.indexOf(LINE_BREAK, offset);
    if (next === -1) {
      return offset;
    }
    offset = next + 1;
  }
  return offset;
}

function lineEndOffset(text: string, start: number): number {
  const next = text.indexOf(LINE_BREAK, start);
  return next === -1 ? text.length : next;
}

export function positionToOffset(text: string, line: number, column: number): number {
  const safeLine = Number.isFinite(line) ? Math.max(1, Math.floor(line)) : 1;
  const safeColumn = Number.isFinite(column) ? Math.max(1, Math.floor(column)) : 1;
  const start = lineStartOffset(text, safeLine);
  const end = lineEndOffset(text, start);
  return Math.min(start + safeColumn - 1, end);
}

export function diagnosticRange(text: string, diagnostic: EditorDiagnostic): TextRange {
  const from = positionToOffset(text, diagnostic.line, diagnostic.column);
  const length = Number.isFinite(diagnostic.length) ? Math.max(MIN_LENGTH, Math.floor(diagnostic.length)) : MIN_LENGTH;
  const to = Math.min(from + length, text.length);
  if (text.slice(from, to).replaceAll(LINE_BREAK, '') !== '') {
    return { from, to };
  }
  const lineStart = lineStartOffset(text, diagnostic.line);
  if (from > lineStart) {
    return { from: from - 1, to: from };
  }
  return { from, to };
}

export function byteLength(text: string): number {
  return new TextEncoder().encode(text).byteLength;
}
