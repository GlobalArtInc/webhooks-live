import { MAX_NESTING_DEPTH } from './constants';
import type { FormatError, FormatKind, FormatResult } from './types';

interface TextPosition {
  line: number;
  column: number;
}

const INDENT = '  ';
const OUTPUT_FACTOR = 8;
const OUTPUT_SLACK = 64 * 1024;
const NUMBER_PATTERN = /-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/y;
const XML_NAME_START = /[A-Za-z_:\u0080-￿]/;
const JSON_ESCAPES = '"\\/bfnrtu';
const HEX_DIGITS = /^[0-9A-Fa-f]{4}$/;
const LITERALS: readonly string[] = ['true', 'false', 'null'];

export function positionAt(text: string, offset: number): TextPosition {
  let line = 1;
  let lineStart = 0;
  let next = text.indexOf('\n');
  while (next !== -1 && next < offset) {
    line += 1;
    lineStart = next + 1;
    next = text.indexOf('\n', lineStart);
  }
  return { line, column: offset - lineStart + 1 };
}

function failure(text: string, offset: number, message: (position: string) => string): FormatResult {
  const { line, column } = positionAt(text, offset);
  const error: FormatError = { message: message(`${line}:${column}`), line, column };
  return { ok: false, error };
}

function syntaxFailure(text: string, offset: number, label: string): FormatResult {
  return failure(text, offset, (position) => `Not valid ${label} at ${position}`);
}

function depthFailure(text: string, offset: number, label: string): FormatResult {
  return failure(text, offset, (position) => `${label} nested deeper than ${MAX_NESTING_DEPTH} levels at ${position}`);
}

function sizeFailure(text: string, label: string): FormatResult {
  return failure(text, text.length, () => `Formatted ${label} is too large to display`);
}

function outputLimit(inputLength: number): number {
  return inputLength * OUTPUT_FACTOR + OUTPUT_SLACK;
}

function isJsonWhitespace(char: string | undefined): boolean {
  return char === ' ' || char === '\t' || char === '\n' || char === '\r';
}

type JsonState =
  'start' | 'arrayFirst' | 'arrayValue' | 'objectFirst' | 'objectKey' | 'colon' | 'objectValue' | 'afterValue';

function scanJsonString(text: string, start: number): number {
  let index = start + 1;
  while (index < text.length) {
    const char = text.charAt(index);
    const code = text.charCodeAt(index);
    if (char === '"') {
      return index + 1;
    }
    if (code < 0x20) {
      return -(index + 1);
    }
    if (char === '\\') {
      const escape = text.charAt(index + 1);
      if (escape === '' || !JSON_ESCAPES.includes(escape)) {
        return -(index + 2);
      }
      if (escape === 'u') {
        if (!HEX_DIGITS.test(text.slice(index + 2, index + 6))) {
          return -(index + 3);
        }
        index += 6;
        continue;
      }
      index += 2;
      continue;
    }
    index += 1;
  }
  return -(text.length + 1);
}

export function formatJson(text: string): FormatResult {
  const limit = outputLimit(text.length);
  const out: string[] = [];
  const containers: string[] = [];
  let outputSize = 0;
  let state: JsonState = 'start';
  let index = 0;

  const emit = (chunk: string): void => {
    out.push(chunk);
    outputSize += chunk.length;
  };
  const newline = (depth: number): void => {
    emit(`\n${INDENT.repeat(depth)}`);
  };

  while (true) {
    while (isJsonWhitespace(text[index])) {
      index += 1;
    }
    if (outputSize > limit) {
      return sizeFailure(text, 'JSON');
    }
    if (index >= text.length) {
      if (state === 'afterValue' && containers.length === 0) {
        return { ok: true, text: out.join('') };
      }
      return syntaxFailure(text, text.length, 'JSON');
    }
    const char = text.charAt(index);

    if (state === 'afterValue') {
      const top = containers[containers.length - 1];
      if (top === undefined) {
        return syntaxFailure(text, index, 'JSON');
      }
      if (char === ',') {
        emit(',');
        newline(containers.length);
        state = top === '[' ? 'arrayValue' : 'objectKey';
        index += 1;
        continue;
      }
      if ((top === '[' && char === ']') || (top === '{' && char === '}')) {
        containers.pop();
        newline(containers.length);
        emit(char);
        index += 1;
        continue;
      }
      return syntaxFailure(text, index, 'JSON');
    }

    if (state === 'colon') {
      if (char !== ':') {
        return syntaxFailure(text, index, 'JSON');
      }
      emit(': ');
      state = 'objectValue';
      index += 1;
      continue;
    }

    if (state === 'objectFirst' || state === 'objectKey') {
      if (state === 'objectFirst' && char === '}') {
        containers.pop();
        emit('}');
        state = 'afterValue';
        index += 1;
        continue;
      }
      if (char !== '"') {
        return syntaxFailure(text, index, 'JSON');
      }
      if (state === 'objectFirst') {
        newline(containers.length);
      }
      const end = scanJsonString(text, index);
      if (end < 0) {
        return syntaxFailure(text, -end - 1, 'JSON');
      }
      emit(text.slice(index, end));
      index = end;
      state = 'colon';
      continue;
    }

    if (state === 'arrayFirst' && char === ']') {
      containers.pop();
      emit(']');
      state = 'afterValue';
      index += 1;
      continue;
    }
    if (state === 'arrayFirst') {
      newline(containers.length);
    }

    if (char === '{' || char === '[') {
      if (containers.length >= MAX_NESTING_DEPTH) {
        return depthFailure(text, index, 'JSON');
      }
      containers.push(char);
      emit(char);
      state = char === '{' ? 'objectFirst' : 'arrayFirst';
      index += 1;
      continue;
    }
    if (char === '"') {
      const end = scanJsonString(text, index);
      if (end < 0) {
        return syntaxFailure(text, -end - 1, 'JSON');
      }
      emit(text.slice(index, end));
      index = end;
      state = 'afterValue';
      continue;
    }
    const literal = LITERALS.find((candidate) => text.startsWith(candidate, index));
    if (literal !== undefined) {
      emit(literal);
      index += literal.length;
      state = 'afterValue';
      continue;
    }
    NUMBER_PATTERN.lastIndex = index;
    const match = NUMBER_PATTERN.exec(text);
    if (match === null) {
      return syntaxFailure(text, index, 'JSON');
    }
    emit(match[0]);
    index += match[0].length;
    state = 'afterValue';
  }
}

type XmlTokenKind = 'open' | 'close' | 'self' | 'text' | 'other';

interface XmlToken {
  kind: XmlTokenKind;
  raw: string;
}

function findXmlTagEnd(text: string, start: number): number {
  let quote = '';
  for (let index = start; index < text.length; index += 1) {
    const char = text.charAt(index);
    if (quote !== '') {
      if (char === quote) {
        quote = '';
      }
    } else if (char === '"' || char === "'") {
      quote = char;
    } else if (char === '>') {
      return index;
    }
  }
  return -1;
}

function findDoctypeEnd(text: string, start: number): number {
  let quote = '';
  let bracketDepth = 0;
  for (let index = start; index < text.length; index += 1) {
    const char = text.charAt(index);
    if (quote !== '') {
      if (char === quote) {
        quote = '';
      }
    } else if (char === '"' || char === "'") {
      quote = char;
    } else if (char === '[') {
      bracketDepth += 1;
    } else if (char === ']') {
      bracketDepth -= 1;
    } else if (char === '>' && bracketDepth <= 0) {
      return index;
    }
  }
  return -1;
}

function xmlTagName(raw: string, from: number): string {
  let end = from;
  while (end < raw.length && !/[\s/>]/.test(raw.charAt(end))) {
    end += 1;
  }
  return raw.slice(from, end);
}

export function formatXml(text: string): FormatResult {
  const limit = outputLimit(text.length);
  const tokens: XmlToken[] = [];
  const names: string[] = [];
  let index = 0;

  while (index < text.length) {
    if (text.charAt(index) !== '<') {
      const next = text.indexOf('<', index);
      const end = next === -1 ? text.length : next;
      const content = text.slice(index, end).trim();
      if (content !== '') {
        tokens.push({ kind: 'text', raw: content });
      }
      index = end;
      continue;
    }

    let end = -1;
    let kind: XmlTokenKind = 'other';
    if (text.startsWith('<!--', index)) {
      const close = text.indexOf('-->', index + 4);
      end = close === -1 ? -1 : close + 3;
    } else if (text.startsWith('<![CDATA[', index)) {
      const close = text.indexOf(']]>', index + 9);
      end = close === -1 ? -1 : close + 3;
      kind = 'text';
    } else if (text.startsWith('<?', index)) {
      const close = text.indexOf('?>', index + 2);
      end = close === -1 ? -1 : close + 2;
    } else if (text.startsWith('<!', index)) {
      const close = findDoctypeEnd(text, index + 2);
      end = close === -1 ? -1 : close + 1;
    } else if (text.startsWith('</', index)) {
      const close = text.indexOf('>', index + 2);
      end = close === -1 ? -1 : close + 1;
      kind = 'close';
    } else {
      const close = findXmlTagEnd(text, index + 1);
      end = close === -1 ? -1 : close + 1;
      kind = close !== -1 && text.charAt(close - 1) === '/' ? 'self' : 'open';
    }
    if (end === -1) {
      return syntaxFailure(text, index, 'XML');
    }

    const raw = text.slice(index, end);
    if (kind === 'open' || kind === 'self') {
      const name = xmlTagName(raw, 1);
      if (!XML_NAME_START.test(name.charAt(0))) {
        return syntaxFailure(text, index, 'XML');
      }
      if (kind === 'open') {
        if (names.length >= MAX_NESTING_DEPTH) {
          return depthFailure(text, index, 'XML');
        }
        names.push(name);
      }
    } else if (kind === 'close') {
      const name = xmlTagName(raw, 2);
      if (names.pop() !== name) {
        return syntaxFailure(text, index, 'XML');
      }
    }
    tokens.push({ kind, raw });
    index = end;
  }

  if (names.length > 0) {
    return syntaxFailure(text, text.length, 'XML');
  }

  const lines: string[] = [];
  let outputSize = 0;
  let depth = 0;
  const line = (value: string): void => {
    const row = INDENT.repeat(depth) + value;
    lines.push(row);
    outputSize += row.length + 1;
  };

  let cursor = 0;
  while (cursor < tokens.length) {
    if (outputSize > limit) {
      return sizeFailure(text, 'XML');
    }
    const token = tokens[cursor];
    if (token === undefined) {
      break;
    }
    if (token.kind === 'open') {
      const next = tokens[cursor + 1];
      const afterNext = tokens[cursor + 2];
      if (next !== undefined && next.kind === 'close') {
        line(token.raw + next.raw);
        cursor += 2;
        continue;
      }
      if (next !== undefined && next.kind === 'text' && afterNext !== undefined && afterNext.kind === 'close') {
        line(token.raw + next.raw + afterNext.raw);
        cursor += 3;
        continue;
      }
      line(token.raw);
      depth += 1;
    } else if (token.kind === 'close') {
      depth -= 1;
      line(token.raw);
    } else {
      line(token.raw);
    }
    cursor += 1;
  }
  return { ok: true, text: lines.join('\n') };
}

export function formatDocument(kind: FormatKind, text: string): FormatResult {
  return kind === 'json' ? formatJson(text) : formatXml(text);
}
