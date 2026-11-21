import { HEX_BYTES, HEX_ROW_BYTES } from './constants';
import type { BodyContent, Bytes } from './types';

const KIB = 1024;
const MIB = KIB * 1024;

export function toBytes(content: BodyContent): Bytes {
  return content.type === 'bytes' ? content.bytes : new TextEncoder().encode(content.text);
}

export function decodeText(bytes: Uint8Array, partial: boolean): string {
  return new TextDecoder('utf-8', { fatal: false }).decode(bytes, { stream: partial });
}

export function formatSize(size: number): string {
  if (size >= MIB) {
    return `${(size / MIB).toFixed(1)} MiB`;
  }
  if (size >= KIB) {
    return `${(size / KIB).toFixed(1)} KiB`;
  }
  return `${size} B`;
}

export function pickLonger(first: Bytes | null, second: Bytes | null): Bytes | null {
  if (first === null) {
    return second;
  }
  if (second === null) {
    return first;
  }
  return second.length > first.length ? second : first;
}

function hexByte(value: number): string {
  return value.toString(16).padStart(2, '0');
}

function printable(value: number): string {
  return value >= 0x20 && value <= 0x7e ? String.fromCharCode(value) : '.';
}

export function formatHex(bytes: Uint8Array, limit: number = HEX_BYTES): string {
  const end = Math.min(bytes.length, limit);
  const rows: string[] = [];
  for (let offset = 0; offset < end; offset += HEX_ROW_BYTES) {
    const slice = bytes.subarray(offset, Math.min(offset + HEX_ROW_BYTES, end));
    const cells: string[] = [];
    for (let column = 0; column < HEX_ROW_BYTES; column += 1) {
      const value = slice[column];
      cells.push(value === undefined ? '  ' : hexByte(value));
      if (column === HEX_ROW_BYTES / 2 - 1) {
        cells.push('');
      }
    }
    const ascii = Array.from(slice, printable).join('');
    rows.push(`${offset.toString(16).padStart(8, '0')}  ${cells.join(' ')}  |${ascii}|`);
  }
  return rows.join('\n');
}
