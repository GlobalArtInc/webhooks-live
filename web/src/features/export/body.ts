import { ExportError } from './errors';
import type { CaptureBody } from './types';

const BASE64_CHUNK = 0x8000;
const encoder = new TextEncoder();
const strictDecoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

export interface DecodedBody {
  bytes: Uint8Array<ArrayBuffer>;
  text: string | null;
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let offset = 0; offset < bytes.length; offset += BASE64_CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + BASE64_CHUNK));
  }
  return btoa(binary);
}

function base64ToBytes(data: string): Uint8Array<ArrayBuffer> {
  let binary: string;
  try {
    binary = atob(data);
  } catch {
    throw new ExportError('Body is not valid base64');
  }
  return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}

function decodeText(bytes: Uint8Array<ArrayBuffer>): string | null {
  if (bytes.includes(0)) {
    return null;
  }
  try {
    return strictDecoder.decode(bytes);
  } catch {
    return null;
  }
}

export function decodeBody(body: CaptureBody): DecodedBody {
  if (body.truncated) {
    throw new ExportError('Body is truncated; fetch the full body before exporting');
  }
  const bytes = body.encoding === 'utf8' ? encoder.encode(body.data) : base64ToBytes(body.data);
  return { bytes, text: decodeText(bytes) };
}
