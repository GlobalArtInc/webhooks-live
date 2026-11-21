import type { Capture, NameValue } from '../../api/types';
import { VOLATILE_HEADER_NAMES } from './constants';
import type { NormaliseOptions, NormalisedCapture } from './types';

export const DEFAULT_NORMALISE_OPTIONS: NormaliseOptions = {
  hideVolatile: true,
  sortHeaders: false,
  sortQuery: false,
  sortJsonKeys: true,
  binaryDigest: null,
};

const JSON_INDENT = '  ';

export function compareText(left: string, right: string): number {
  if (left < right) {
    return -1;
  }
  return left > right ? 1 : 0;
}

export function isVolatileHeader(name: string): boolean {
  const lower = name.toLowerCase();
  return VOLATILE_HEADER_NAMES.some((pattern) =>
    pattern.endsWith('*') ? lower.startsWith(pattern.slice(0, -1)) : lower === pattern,
  );
}

function headerRowsFor(headers: NameValue[], options: NormaliseOptions): NameValue[] {
  const kept = headers.filter((header) => !options.hideVolatile || !isVolatileHeader(header.name));
  return options.sortHeaders
    ? kept.sort((left, right) => compareText(left.name.toLowerCase(), right.name.toLowerCase()))
    : kept;
}

function queryText(query: NameValue[], sort: boolean): string {
  const pairs = query.map((pair) => `${pair.name}=${pair.value}`);
  if (sort) {
    pairs.sort(compareText);
  }
  return pairs.join('&');
}

function prettyJson(value: unknown, sortKeys: boolean, indent: string): string {
  const inner = `${indent}${JSON_INDENT}`;
  if (Array.isArray(value)) {
    if (value.length === 0) {
      return '[]';
    }
    const items = value.map((item) => `${inner}${prettyJson(item, sortKeys, inner)}`);
    return `[\n${items.join(',\n')}\n${indent}]`;
  }
  if (typeof value === 'object' && value !== null) {
    const entries: [string, unknown][] = Object.entries(value);
    if (entries.length === 0) {
      return '{}';
    }
    if (sortKeys) {
      entries.sort(([left], [right]) => compareText(left, right));
    }
    const members = entries.map(
      ([key, item]) => `${inner}${JSON.stringify(key)}: ${prettyJson(item, sortKeys, inner)}`,
    );
    return `{\n${members.join(',\n')}\n${indent}}`;
  }
  return JSON.stringify(value);
}

function textBody(text: string, sortKeys: boolean): string {
  const trimmed = text.trim();
  if (!trimmed.startsWith('{') && !trimmed.startsWith('[')) {
    return text;
  }
  try {
    const parsed: unknown = JSON.parse(trimmed);
    return prettyJson(parsed, sortKeys, '');
  } catch {
    return text;
  }
}

function binaryBody(size: number, digest: string | null): string {
  const lines = [`size: ${size} bytes`];
  if (digest !== null) {
    lines.push(`sha256: ${digest}`);
  }
  return lines.join('\n');
}

export function normaliseCapture(capture: Capture, options: NormaliseOptions): NormalisedCapture {
  const binary = capture.body.encoding === 'base64';
  const headerRows = headerRowsFor(capture.headers, options);
  return {
    summary: {
      method: capture.method,
      path: capture.path,
      query: queryText(capture.query, options.sortQuery),
    },
    headers: headerRows.map((header) => `${header.name}: ${header.value}`).join('\n'),
    headerRows,
    body: binary
      ? binaryBody(capture.body_size, options.binaryDigest)
      : textBody(capture.body.data, options.sortJsonKeys),
    binary,
  };
}
