import { MAX_ESCAPED_CONTROL_CHARS, OMITTED_REQUEST_HEADERS } from './constants';
import { countTerminalHazards } from './shellQuote';
import type { NameValue, SkippedHeader } from './types';

export const HTTP_TOKEN_PATTERN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
const BLANK_VALUE_PATTERN = /^[ \t]*$/;
const UNSENDABLE_VALUE_CHARS: readonly string[] = ['\r', '\n', '\u0000'];

export interface HeaderSplit {
  sendable: NameValue[];
  skipped: SkippedHeader[];
}

export function isBlankHeaderValue(value: string): boolean {
  return BLANK_VALUE_PATTERN.test(value);
}

export function findHeaderValue(headers: readonly NameValue[], name: string): string | null {
  const wanted = name.toLowerCase();
  const found = headers.find((header) => header.name.toLowerCase() === wanted);
  return found === undefined ? null : found.value;
}

function isUnsendable(header: NameValue): boolean {
  return (
    !HTTP_TOKEN_PATTERN.test(header.name) ||
    UNSENDABLE_VALUE_CHARS.some((char) => header.value.includes(char)) ||
    countTerminalHazards(header.value) > MAX_ESCAPED_CONTROL_CHARS
  );
}

export function splitHeaders(headers: readonly NameValue[]): HeaderSplit {
  const sendable: NameValue[] = [];
  const skipped: SkippedHeader[] = [];
  for (const header of headers) {
    if (OMITTED_REQUEST_HEADERS.includes(header.name.toLowerCase())) {
      skipped.push({ name: header.name, value: header.value, reason: 'omitted' });
    } else if (isUnsendable(header)) {
      skipped.push({ name: header.name, value: header.value, reason: 'unsendable' });
    } else {
      sendable.push(header);
    }
  }
  return { sendable, skipped };
}
