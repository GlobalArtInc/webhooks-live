import { LARGE_BODY_LIMIT, SNIFF_BYTES } from './constants';
import type { BodyClass, Bytes, ImageType, LargeHint, TextKind } from './types';

const IMAGE_MIME: Readonly<Record<ImageType, string>> = {
  png: 'image/png',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
};

const PNG_MAGIC = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const JPEG_MAGIC = [0xff, 0xd8, 0xff];
const GIF87_MAGIC = [0x47, 0x49, 0x46, 0x38, 0x37, 0x61];
const GIF89_MAGIC = [0x47, 0x49, 0x46, 0x38, 0x39, 0x61];
const RIFF_MAGIC = [0x52, 0x49, 0x46, 0x46];
const WEBP_MAGIC = [0x57, 0x45, 0x42, 0x50];
const UTF8_BOM = [0xef, 0xbb, 0xbf];
const WHITESPACE = [0x20, 0x09, 0x0a, 0x0d];
const OPEN_BRACE = 0x7b;
const OPEN_BRACKET = 0x5b;

export function imageMimeType(image: ImageType): string {
  return IMAGE_MIME[image];
}

function hasMagic(bytes: Uint8Array, magic: readonly number[], offset: number): boolean {
  if (bytes.length < offset + magic.length) {
    return false;
  }
  return magic.every((value, index) => bytes[offset + index] === value);
}

export function sniffImageType(bytes: Uint8Array): ImageType | null {
  if (hasMagic(bytes, PNG_MAGIC, 0)) {
    return 'png';
  }
  if (hasMagic(bytes, JPEG_MAGIC, 0)) {
    return 'jpeg';
  }
  if (hasMagic(bytes, GIF87_MAGIC, 0) || hasMagic(bytes, GIF89_MAGIC, 0)) {
    return 'gif';
  }
  if (hasMagic(bytes, RIFF_MAGIC, 0) && hasMagic(bytes, WEBP_MAGIC, 8)) {
    return 'webp';
  }
  return null;
}

export function isBinaryPrefix(bytes: Uint8Array): boolean {
  const truncated = bytes.length > SNIFF_BYTES;
  const prefix = truncated ? bytes.subarray(0, SNIFF_BYTES) : bytes;
  if (prefix.includes(0)) {
    return true;
  }
  try {
    new TextDecoder('utf-8', { fatal: true }).decode(prefix, { stream: truncated });
  } catch (error) {
    if (error instanceof TypeError) {
      return true;
    }
    throw error;
  }
  return false;
}

function mimeEssence(contentType: string): string {
  const [essence = ''] = contentType.split(';');
  return essence.trim().toLowerCase();
}

function headerKind(contentType: string): TextKind | null {
  const essence = mimeEssence(contentType);
  if (essence === 'image/svg+xml') {
    return 'text';
  }
  if (essence === 'application/json' || essence.endsWith('+json')) {
    return 'json';
  }
  if (essence.includes('xml')) {
    return 'xml';
  }
  if (essence === 'text/html') {
    return 'html';
  }
  return null;
}

function startsLikeJson(bytes: Uint8Array): boolean {
  let index = hasMagic(bytes, UTF8_BOM, 0) ? UTF8_BOM.length : 0;
  while (index < bytes.length && index < SNIFF_BYTES && WHITESPACE.includes(bytes[index] ?? 0)) {
    index += 1;
  }
  const first = bytes[index];
  return first === OPEN_BRACE || first === OPEN_BRACKET;
}

function parsesAsJson(bytes: Uint8Array): boolean {
  const text = new TextDecoder('utf-8', { fatal: false }).decode(bytes);
  try {
    JSON.parse(text);
  } catch (error) {
    if (error instanceof SyntaxError) {
      return false;
    }
    throw error;
  }
  return true;
}

function classifyPrefix(bytes: Uint8Array, contentType: string, verifySniff: boolean): LargeHint {
  const image = sniffImageType(bytes);
  if (image !== null) {
    return { kind: 'image', image };
  }
  if (isBinaryPrefix(bytes)) {
    return { kind: 'binary' };
  }
  const fromHeader = headerKind(contentType);
  if (fromHeader !== null) {
    return { kind: fromHeader };
  }
  if (startsLikeJson(bytes) && (!verifySniff || parsesAsJson(bytes))) {
    return { kind: 'json' };
  }
  return { kind: 'text' };
}

export function classifyBody(bytes: Bytes, contentType: string, totalSize: number = bytes.length): BodyClass {
  if (totalSize > LARGE_BODY_LIMIT) {
    return { kind: 'large', hint: classifyPrefix(bytes, contentType, false) };
  }
  return classifyPrefix(bytes, contentType, true);
}
