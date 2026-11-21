import { ELLIPSIS } from './constants';
import type { InertSegment } from './types';

const UNSAFE = /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}\p{Zs}\p{Cs}\u115F\u1160\u17B4\u17B5\u2800\u3164\uFFA0]/gu;
const SPACE = 0x20;
const ZERO_WIDTH_JOINER = 0x200d;
const ZERO_WIDTH_NON_JOINER = 0x200c;
const TAB = 9;
const LINE_FEED = 10;
const CARRIAGE_RETURN = 13;
const HIGH_SURROGATE_START = 0xd800;
const HIGH_SURROGATE_END = 0xdbff;
const HEX_RADIX = 16;
const HEX_WIDTH = 4;
const NAMED: Readonly<Record<number, string>> = {
  [TAB]: '\\t',
  [LINE_FEED]: '\\n',
  [CARRIAGE_RETURN]: '\\r',
};

function replacementFor(char: string, keepLayout: boolean): string | null {
  const code = char.codePointAt(0) ?? 0;
  if (code === SPACE || code === ZERO_WIDTH_JOINER || code === ZERO_WIDTH_NON_JOINER) {
    return null;
  }
  if (keepLayout && (code === LINE_FEED || code === TAB)) {
    return null;
  }
  return NAMED[code] ?? `\\u{${code.toString(HEX_RADIX).toUpperCase().padStart(HEX_WIDTH, '0')}}`;
}

export function escapeInert(text: string): string {
  return text.replace(UNSAFE, (char) => replacementFor(char, false) ?? char);
}

export function escapeMultiline(text: string): string {
  return text.replace(/\r\n/g, '\n').replace(UNSAFE, (char) => replacementFor(char, true) ?? char);
}

export function segmentInert(text: string): InertSegment[] {
  const segments: InertSegment[] = [];
  let cursor = 0;
  for (const match of text.matchAll(UNSAFE)) {
    const replacement = replacementFor(match[0], false);
    if (replacement === null) {
      continue;
    }
    if (match.index > cursor) {
      segments.push({ text: text.slice(cursor, match.index), escaped: false });
    }
    segments.push({ text: replacement, escaped: true });
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) {
    segments.push({ text: text.slice(cursor), escaped: false });
  }
  return segments;
}

export function truncateText(value: string, end: number): string {
  if (end <= 0) {
    return '';
  }
  const last = value.charCodeAt(end - 1);
  const safeEnd = last >= HIGH_SURROGATE_START && last <= HIGH_SURROGATE_END ? end - 1 : end;
  return value.slice(0, safeEnd);
}

export function clampText(value: string, limit: number): string {
  if (value.length <= limit) {
    return value;
  }
  return `${truncateText(value, limit)}${ELLIPSIS}`;
}

export function unfoldHeader(value: string): string {
  return value.replace(/\r?\n[ \t]+/g, ' ');
}
