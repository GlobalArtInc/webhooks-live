import type { Bytes, FormatError, FormatKind, FormatRequest, FormatResponse, FormatResult } from './types';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isFormatKind(value: unknown): value is FormatKind {
  return value === 'json' || value === 'xml';
}

function isBytes(value: unknown): value is Bytes {
  return value instanceof Uint8Array && value.buffer instanceof ArrayBuffer;
}

function isFormatError(value: unknown): value is FormatError {
  return (
    isRecord(value) &&
    typeof value['message'] === 'string' &&
    typeof value['line'] === 'number' &&
    typeof value['column'] === 'number'
  );
}

function isFormatResult(value: unknown): value is FormatResult {
  if (!isRecord(value)) {
    return false;
  }
  if (value['ok'] === true) {
    return typeof value['text'] === 'string';
  }
  return value['ok'] === false && isFormatError(value['error']);
}

export function isFormatRequest(value: unknown): value is FormatRequest {
  return isRecord(value) && typeof value['id'] === 'number' && isFormatKind(value['kind']) && isBytes(value['bytes']);
}

export function isFormatResponse(value: unknown): value is FormatResponse {
  return isRecord(value) && typeof value['id'] === 'number' && isFormatResult(value['result']);
}
