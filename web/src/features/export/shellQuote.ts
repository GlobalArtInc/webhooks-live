import { ExportError } from './errors';

const encoder = new TextEncoder();

interface QuoteSegment {
  hazard: boolean;
  text: string;
}

function isTerminalHazard(codePoint: number): boolean {
  return (
    codePoint <= 0x08 ||
    (codePoint >= 0x0b && codePoint <= 0x1f) ||
    (codePoint >= 0x7f && codePoint <= 0x9f) ||
    codePoint === 0x061c ||
    codePoint === 0x200e ||
    codePoint === 0x200f ||
    (codePoint >= 0x202a && codePoint <= 0x202e) ||
    (codePoint >= 0x2066 && codePoint <= 0x2069)
  );
}

function splitSegments(value: string): QuoteSegment[] {
  const segments: QuoteSegment[] = [];
  for (const char of value) {
    const hazard = isTerminalHazard(char.codePointAt(0) ?? 0);
    const last = segments[segments.length - 1];
    if (last !== undefined && last.hazard === hazard) {
      last.text += char;
    } else {
      segments.push({ hazard, text: char });
    }
  }
  return segments;
}

export function toPrintfOctal(bytes: Uint8Array): string {
  let out = '';
  for (const byte of bytes) {
    out += `\\${byte.toString(8).padStart(3, '0')}`;
  }
  return out;
}

function isLiteralPrintfByte(byte: number): boolean {
  return byte >= 0x20 && byte <= 0x7e && byte !== 0x25 && byte !== 0x27 && byte !== 0x5c;
}

function isDigitByte(byte: number): boolean {
  return byte >= 0x30 && byte <= 0x39;
}

export function toPrintfFormat(bytes: Uint8Array): string {
  const parts: string[] = [];
  let afterEscape = false;
  for (const byte of bytes) {
    const leadingDash = parts.length === 0 && byte === 0x2d;
    if (isLiteralPrintfByte(byte) && !leadingDash && !(afterEscape && isDigitByte(byte))) {
      parts.push(String.fromCharCode(byte));
      afterEscape = false;
    } else {
      parts.push(`\\${byte.toString(8).padStart(3, '0')}`);
      afterEscape = true;
    }
  }
  return parts.join('');
}

export function countTerminalHazards(value: string): number {
  let count = 0;
  for (const char of value) {
    if (isTerminalHazard(char.codePointAt(0) ?? 0)) {
      count += 1;
    }
  }
  return count;
}

export function singleQuote(text: string): string {
  return `'${text.replaceAll("'", "'\\''")}'`;
}

function quoteSegment(segment: QuoteSegment): string {
  if (!segment.hazard) {
    return singleQuote(segment.text);
  }
  return `"$(printf ${singleQuote(toPrintfOctal(encoder.encode(segment.text)))})"`;
}

export function shellQuote(value: string): string {
  if (value.includes('\u0000')) {
    throw new ExportError('A NUL character cannot be passed as a shell argument');
  }
  if (value === '') {
    return "''";
  }
  return splitSegments(value).map(quoteSegment).join('');
}
