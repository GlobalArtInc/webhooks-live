export const GENERATED_SECRET_LENGTH = 32;
export const SECRET_MAX_BYTES = 256;
export const PASSWORD_MIN = 8;
export const PASSWORD_MAX = 128;

const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
const FIRST_VISIBLE = 0x20;
const LAST_VISIBLE = 0x7e;
const HEADER_TOKEN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;

export type RandomBytes = (length: number) => Uint8Array;

function browserRandomBytes(length: number): Uint8Array {
  return globalThis.crypto.getRandomValues(new Uint8Array(length));
}

export function generateSecret(randomBytes: RandomBytes = browserRandomBytes): string {
  const bytes = randomBytes(GENERATED_SECRET_LENGTH);
  return Array.from(bytes, (byte) => ALPHABET.charAt(byte % ALPHABET.length)).join('');
}

export function isVisibleAscii(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < FIRST_VISIBLE || code > LAST_VISIBLE) {
      return false;
    }
  }
  return true;
}

export function isHeaderName(value: string): boolean {
  return HEADER_TOKEN.test(value);
}
