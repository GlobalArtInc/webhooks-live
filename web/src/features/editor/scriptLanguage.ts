import { StreamLanguage, type StreamParser, type StringStream } from '@codemirror/language';

export const DEFAULT_SCRIPT_KEYWORDS: readonly string[] = [
  'let',
  'const',
  'if',
  'else',
  'switch',
  'while',
  'loop',
  'do',
  'until',
  'for',
  'in',
  'fn',
  'return',
  'break',
  'continue',
  'throw',
  'try',
  'catch',
  'import',
  'export',
  'as',
  'private',
];

const LITERALS: readonly string[] = ['true', 'false'];
const QUOTES: readonly string[] = ['"', "'", '`'];
const NUMBER = /^(?:0x[0-9a-fA-F_]+|0b[01_]+|0o[0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)/;
const WORD = /^[A-Za-z_][A-Za-z0-9_]*/;

interface ScriptState {
  quote: string | null;
  comment: boolean;
}

function readString(stream: StringStream, state: ScriptState): void {
  const quote = state.quote;
  while (!stream.eol()) {
    const next = stream.next();
    if (next === '\\') {
      stream.next();
    } else if (next === quote) {
      state.quote = null;
      return;
    }
  }
}

function readComment(stream: StringStream, state: ScriptState): void {
  while (!stream.eol()) {
    if (stream.match('*/')) {
      state.comment = false;
      return;
    }
    stream.next();
  }
}

function buildParser(keywords: readonly string[]): StreamParser<ScriptState> {
  const keywordSet = new Set(keywords);
  return {
    name: 'script',
    startState: () => ({ quote: null, comment: false }),
    token: (stream, state) => {
      if (state.comment) {
        readComment(stream, state);
        return 'comment';
      }
      if (state.quote !== null) {
        readString(stream, state);
        return 'string';
      }
      if (stream.eatSpace()) {
        return null;
      }
      if (stream.match('//')) {
        stream.skipToEnd();
        return 'comment';
      }
      if (stream.match('/*')) {
        state.comment = true;
        readComment(stream, state);
        return 'comment';
      }
      const next = stream.peek();
      if (next !== undefined && QUOTES.includes(next)) {
        state.quote = next;
        stream.next();
        readString(stream, state);
        return 'string';
      }
      if (stream.match(NUMBER)) {
        return 'number';
      }
      const word = stream.match(WORD);
      if (Array.isArray(word)) {
        const text = word[0];
        if (LITERALS.includes(text)) {
          return 'bool';
        }
        return keywordSet.has(text) ? 'keyword' : null;
      }
      stream.next();
      return null;
    },
    languageData: { commentTokens: { line: '//', block: { open: '/*', close: '*/' } } },
  };
}

export function scriptLanguage(keywords: readonly string[] | null) {
  return StreamLanguage.define(buildParser(keywords ?? DEFAULT_SCRIPT_KEYWORDS));
}
