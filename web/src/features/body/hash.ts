import type { Bytes } from './types';

export async function sha256Hex(bytes: Bytes): Promise<string> {
  if (typeof crypto === 'undefined' || typeof crypto.subtle === 'undefined') {
    throw new Error('Web Crypto is unavailable in this context (HTTPS is required)');
  }
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return Array.from(new Uint8Array(digest), (value) => value.toString(16).padStart(2, '0')).join('');
}
