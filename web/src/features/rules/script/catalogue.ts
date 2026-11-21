import type { CompletionEntry, CompletionSource } from '../../editor/types';
import type { HoverEntry, HoverTarget, ScriptLanguageMeta, ScriptPhase, ScriptPhaseLimits } from './types';

const CHAIN = /[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*/g;
const SEGMENT = /[A-Za-z_][A-Za-z0-9_]*/g;
const QUOTES: readonly string[] = ['"', "'", '`'];
const LINE_COMMENT = '//';

export function scriptCompletionEntries(meta: ScriptLanguageMeta): CompletionEntry[] {
  const functions = meta.functions.map((item): CompletionEntry => ({
    label: item.name,
    insertText: `${item.name}(`,
    detail: item.signature,
    description: item.doc,
    kind: 'function',
  }));
  const variables = meta.variables.map((item): CompletionEntry => ({
    label: item.name,
    detail: item.type,
    description: item.doc,
    kind: 'variable',
  }));
  const keywords = meta.keywords.map((word): CompletionEntry => ({ label: word, kind: 'keyword' }));
  return [...functions, ...variables, ...keywords];
}

export function scriptCompletionSource(meta: ScriptLanguageMeta): CompletionSource {
  const entries = scriptCompletionEntries(meta);
  return () => entries;
}

function isCodeAt(line: string, offset: number): boolean {
  let quote: string | null = null;
  for (let index = 0; index < offset; index += 1) {
    const char = line.charAt(index);
    if (quote !== null) {
      if (char === '\\') {
        index += 1;
      } else if (char === quote) {
        quote = null;
      }
    } else if (QUOTES.includes(char)) {
      quote = char;
    } else if (line.startsWith(LINE_COMMENT, index)) {
      return false;
    }
  }
  return quote === null;
}

function lookup(meta: ScriptLanguageMeta, path: string, segment: string, called: boolean): HoverEntry | null {
  const variable = meta.variables.find((item) => item.name === path);
  if (variable !== undefined) {
    return { kind: 'variable', title: variable.name, detail: variable.type, doc: variable.doc };
  }
  if (called) {
    const fn = meta.functions.find((item) => item.name === segment);
    if (fn !== undefined) {
      return { kind: 'function', title: fn.name, detail: fn.signature, doc: fn.doc };
    }
  }
  return null;
}

export function findHoverTarget(line: string, offset: number, meta: ScriptLanguageMeta): HoverTarget | null {
  if (!isCodeAt(line, offset)) {
    return null;
  }
  for (const chain of line.matchAll(CHAIN)) {
    const start = chain.index;
    const end = start + chain[0].length;
    if (offset < start || offset >= end) {
      continue;
    }
    for (const segment of chain[0].matchAll(SEGMENT)) {
      const from = start + segment.index;
      const to = from + segment[0].length;
      if (offset < from || offset >= to) {
        continue;
      }
      const path = line.slice(start, to);
      const called = line.charAt(to) === '(';
      const entry = lookup(meta, path, segment[0], called);
      return entry === null ? null : { from, to, entry };
    }
  }
  return null;
}

export function limitsFor(meta: ScriptLanguageMeta, phase: ScriptPhase): ScriptPhaseLimits {
  return phase === 'before' ? meta.limits.before_response : meta.limits.after_response;
}

export function formatCount(value: number, locale: string): string {
  return new Intl.NumberFormat(locale).format(value);
}

export function formatDurationMs(milliseconds: number, locale: string): string {
  if (milliseconds >= 1000 && milliseconds % 1000 === 0) {
    return new Intl.NumberFormat(locale, { style: 'unit', unit: 'second', unitDisplay: 'short' }).format(
      milliseconds / 1000,
    );
  }
  return new Intl.NumberFormat(locale, { style: 'unit', unit: 'millisecond', unitDisplay: 'short' }).format(
    milliseconds,
  );
}
