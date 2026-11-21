import { decodeBody } from './body';
import { DEFAULT_MIME_TYPE, FILE_EXTENSIONS } from './constants';
import type { CaptureExportInput, ExportFileKind } from './types';

const UNSAFE_FILE_CHARS = /[^A-Za-z0-9._-]/gu;
const MAX_METHOD_LENGTH = 16;
const SHORT_ID_LENGTH = 8;

function sanitize(part: string, fallback: string): string {
  const cleaned = part.replace(UNSAFE_FILE_CHARS, '_');
  return cleaned === '' ? fallback : cleaned;
}

export function shortCaptureId(capture: CaptureExportInput): string {
  return sanitize(capture.id.slice(0, SHORT_ID_LENGTH), 'capture');
}

export function rawBodyBlob(capture: CaptureExportInput): Blob {
  return new Blob([decodeBody(capture.body).bytes], { type: DEFAULT_MIME_TYPE });
}

export function suggestedFileName(capture: CaptureExportInput, kind: ExportFileKind): string {
  const method = sanitize(capture.method.slice(0, MAX_METHOD_LENGTH).toLowerCase(), 'request');
  const shortId = shortCaptureId(capture);
  const timestamp = sanitize(capture.received_at, 'unknown-time');
  return `${method}-${shortId}-${timestamp}.${FILE_EXTENSIONS[kind]}`;
}
