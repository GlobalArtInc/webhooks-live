import type { ImageType } from '../body/types';
import { MAX_EXTENSION_CHARS, MAX_FILE_NAME_CHARS } from './constants';
import { truncateText } from './inert';
import type { EmailPart } from './types';

const RASTER_TYPES: Readonly<Record<string, ImageType>> = {
  'image/png': 'png',
  'image/jpeg': 'jpeg',
  'image/gif': 'gif',
  'image/webp': 'webp',
};

const TEXT_TYPE = 'text/plain';
const FORBIDDEN_NAME_CHARS = /[\\/:*?"<>|]/g;
const INVISIBLE_FILLERS = /[\u115F\u1160\u17B4\u17B5\u2800\u3164\uFFA0]/g;
const CONTROL_AND_FORMAT_CHARS = /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}\p{Cs}]/gu;
const WHITESPACE_RUN = /\s+/g;
const LEADING_JUNK = /^[.\s]+/;
const TRAILING_JUNK = /[.\s]+$/;
const EXTENSION = /\.[A-Za-z0-9]{1,16}$/;
const RESERVED_NAME = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i;

export function baseMediaType(contentType: string): string {
  const [base = ''] = contentType.split(';');
  return base.trim().toLowerCase();
}

export function rasterTypeOf(contentType: string): ImageType | null {
  return RASTER_TYPES[baseMediaType(contentType)] ?? null;
}

export function safeFileName(raw: string | null, fallback: string): string {
  if (raw === null) {
    return fallback;
  }
  let name = raw
    .replace(CONTROL_AND_FORMAT_CHARS, '')
    .replace(INVISIBLE_FILLERS, '_')
    .replace(FORBIDDEN_NAME_CHARS, '_')
    .replace(WHITESPACE_RUN, ' ')
    .replace(LEADING_JUNK, '')
    .replace(TRAILING_JUNK, '');
  if (name.length > MAX_FILE_NAME_CHARS) {
    const extension = EXTENSION.exec(name)?.[0] ?? '';
    const keep = MAX_FILE_NAME_CHARS - Math.min(extension.length, MAX_EXTENSION_CHARS + 1);
    name = `${truncateText(name, keep).replace(TRAILING_JUNK, '')}${extension}`;
  }
  if (name === '') {
    return fallback;
  }
  return RESERVED_NAME.test(name) ? `_${name}` : name;
}

function isBodyText(part: EmailPart): boolean {
  return (
    part.is_text &&
    baseMediaType(part.content_type) === TEXT_TYPE &&
    part.disposition !== 'attachment' &&
    part.filename === null
  );
}

export function pickTextPart(parts: readonly EmailPart[]): EmailPart | null {
  return parts.find(isBodyText) ?? null;
}

export function listedParts(parts: readonly EmailPart[], textPart: EmailPart | null): EmailPart[] {
  return parts.filter((part) => part !== textPart);
}

export function canPreview(part: EmailPart, limit: number): boolean {
  return rasterTypeOf(part.content_type) !== null && part.size > 0 && part.size <= limit;
}
