import type { Capture, NameValue } from '../../api/types';
import { DEFAULT_PART_TYPE, EMAIL_KIND, MAX_FIELD_CHARS, MAX_LIST_ITEMS, MAX_NAME_CHARS, MAX_PARTS } from './constants';
import { clampText } from './inert';
import type { EmailMeta, EmailPart, EmailView, ParseIssue, PartDisposition } from './types';

const LIST_SCAN_FACTOR = 10;

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function readText(value: unknown, limit: number): string | null {
  return typeof value === 'string' ? clampText(value, limit) : null;
}

function readTextList(value: unknown): string[] {
  const items: unknown[] = Array.isArray(value) ? value.slice(0, MAX_LIST_ITEMS * LIST_SCAN_FACTOR) : [value];
  const texts: string[] = [];
  for (const item of items) {
    const text = readText(item, MAX_FIELD_CHARS);
    if (text !== null && text !== '') {
      texts.push(text);
    }
    if (texts.length >= MAX_LIST_ITEMS) {
      break;
    }
  }
  return texts;
}

function readCount(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function readDisposition(value: unknown): PartDisposition | null {
  return value === 'inline' || value === 'attachment' ? value : null;
}

function readPart(value: unknown): EmailPart | null {
  if (!isRecord(value)) {
    return null;
  }
  const index = readCount(value.index);
  if (index === null) {
    return null;
  }
  return {
    index,
    content_type: readText(value.content_type, MAX_NAME_CHARS) ?? DEFAULT_PART_TYPE,
    filename: readText(value.filename, MAX_NAME_CHARS),
    size: readCount(value.size) ?? 0,
    disposition: readDisposition(value.disposition),
    is_text: value.is_text === true,
  };
}

function readParts(value: unknown): EmailPart[] {
  const items: unknown[] = Array.isArray(value) ? value.slice(0, MAX_PARTS) : [];
  const parts: EmailPart[] = [];
  for (const item of items) {
    const part = readPart(item);
    if (part !== null && !parts.some((known) => known.index === part.index)) {
      parts.push(part);
    }
  }
  return parts;
}

function readParseIssue(value: unknown): ParseIssue | null {
  if (typeof value === 'string' && value !== '') {
    return { detail: clampText(value, MAX_FIELD_CHARS) };
  }
  return value === true ? { detail: null } : null;
}

function readMeta(value: unknown): EmailMeta {
  const meta: UnknownRecord = isRecord(value) ? value : {};
  return {
    envelope_from: readText(meta.envelope_from, MAX_FIELD_CHARS),
    envelope_to: readTextList(meta.envelope_to),
    helo: readText(meta.helo, MAX_FIELD_CHARS),
    tls: meta.tls === true,
    tag: readText(meta.tag, MAX_FIELD_CHARS),
    subject: readText(meta.subject, MAX_FIELD_CHARS),
    from: readTextList(meta.from),
    to: readTextList(meta.to),
    cc: readTextList(meta.cc),
    date: readText(meta.date, MAX_NAME_CHARS),
    message_id: readText(meta.message_id, MAX_FIELD_CHARS),
    parts: readParts(meta.parts),
    parse_error: readParseIssue(meta.parse_error),
    parts_truncated: meta.parts_truncated === true,
  };
}

export function readEmailView(capture: Capture): EmailView | null {
  if (!('kind' in capture) || capture.kind !== EMAIL_KIND) {
    return null;
  }
  return { capture, meta: readMeta('meta' in capture ? capture.meta : undefined) };
}

export function findHeader(headers: readonly NameValue[], name: string): string | null {
  const wanted = name.toLowerCase();
  const found = headers.find((header) => header.name.toLowerCase() === wanted);
  return found === undefined ? null : found.value;
}
