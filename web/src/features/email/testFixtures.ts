import type { Capture } from '../../api/types';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { encode, PNG_1X1 } from '../body/fixtures';
import type { Bytes } from '../body/types';

const RLO = '\u202E';
const PDI = '\u2069';
const MIB = 1024 * 1024;

export const EMAIL_CAPTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5e01';
export const HOSTILE_SUBJECT = `Reset ${RLO}drowssap${PDI} <img src=x onerror=alert(1)> now`;
export const HOSTILE_FILE_NAME = `../../etc/${RLO}gpj.exe`;
export const SAFE_FILE_NAME = '_.._etc_gpj.exe';
export const TEXT_PART_BODY = `Hello ${RLO}there\r\nSecond line with <script>alert(1)</script>\r\n`;
export const FAKE_IMAGE_BODY: Bytes = encode('<html><script>alert(1)</script></html>');
export const PNG_BODY: Bytes = PNG_1X1;
export const PNG_PART_INDEX = 3;
export const FAKE_PNG_PART_INDEX = 7;
export const SVG_PART_INDEX = 4;
export const HUGE_PART_INDEX = 5;
export const DOCUMENT_PART_INDEX = 2;
export const BOMB_PART_INDEX = 8;

const RAW_SOURCE = [
  'From: Shop <no-reply@shop.example>',
  'To: k7qm2xta9vnc@in.hooks.example',
  `Subject: ${HOSTILE_SUBJECT}`,
  'Content-Type: multipart/mixed; boundary="wl"',
  '',
  `--wl${RLO}`,
  'Content-Type: text/plain',
  '',
  'Hello',
  '--wl--',
  '',
].join('\r\n');

function be16(value: number): number[] {
  return [(value >> 8) & 0xff, value & 0xff];
}

function be32(value: number): number[] {
  return [...be16(Math.floor(value / 0x10000)), ...be16(value % 0x10000)];
}

function le16(value: number): number[] {
  return [value & 0xff, (value >> 8) & 0xff];
}

function padded(head: number[], total: number): Bytes {
  const bytes = new Uint8Array(Math.max(total, head.length));
  bytes.set(head);
  return bytes;
}

function ascii(text: string): number[] {
  return Array.from(text, (char) => char.charCodeAt(0));
}

export function craftPng(width: number, height: number): Bytes {
  const signature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  return padded([...signature, ...be32(13), ...ascii('IHDR'), ...be32(width), ...be32(height)], 40);
}

export function craftGif(width: number, height: number): Bytes {
  return padded([...ascii('GIF89a'), ...le16(width), ...le16(height)], 16);
}

export function craftJpeg(width: number, height: number): Bytes {
  const app0 = [0xff, 0xe0, ...be16(16), ...new Array<number>(14).fill(0)];
  const frame = [0xff, 0xc0, ...be16(17), 8, ...be16(height), ...be16(width), 3];
  return padded([0xff, 0xd8, ...app0, ...frame], 48);
}

export function craftWebp(kind: 'lossy' | 'lossless' | 'extended', width: number, height: number): Bytes {
  const riff = [...ascii('RIFF'), 0, 0, 0, 0, ...ascii('WEBP')];
  if (kind === 'lossy') {
    return padded(
      [...riff, ...ascii('VP8 '), 0, 0, 0, 0, 0, 0, 0, 0x9d, 0x01, 0x2a, ...le16(width), ...le16(height)],
      40,
    );
  }
  if (kind === 'lossless') {
    const bits = width - 1 + (height - 1) * 0x4000;
    const packed = [bits & 0xff, (bits >> 8) & 0xff, (bits >> 16) & 0xff, Math.floor(bits / 0x1000000) & 0xff];
    return padded([...riff, ...ascii('VP8L'), 0, 0, 0, 0, 0x2f, ...packed], 40);
  }
  const w = width - 1;
  const h = height - 1;
  const dims = [w & 0xff, (w >> 8) & 0xff, (w >> 16) & 0xff, h & 0xff, (h >> 8) & 0xff, (h >> 16) & 0xff];
  return padded([...riff, ...ascii('VP8X'), 0, 0, 0, 0, 0, 0, 0, 0, ...dims], 40);
}

export const BOMB_PNG_BODY: Bytes = craftPng(10_000, 10_000);

const PARTS = [
  {
    index: 0,
    content_type: 'text/plain; charset=utf-8',
    filename: null,
    size: encode(TEXT_PART_BODY).length,
    disposition: null,
    is_text: true,
  },
  { index: 1, content_type: 'text/html', filename: null, size: 400, disposition: null, is_text: true },
  {
    index: DOCUMENT_PART_INDEX,
    content_type: 'application/pdf',
    filename: HOSTILE_FILE_NAME,
    size: 2048,
    disposition: 'attachment',
    is_text: false,
  },
  {
    index: PNG_PART_INDEX,
    content_type: 'image/png',
    filename: 'logo.png',
    size: PNG_1X1.length,
    disposition: 'inline',
    is_text: false,
  },
  {
    index: SVG_PART_INDEX,
    content_type: 'image/svg+xml',
    filename: 'diagram.svg',
    size: 120,
    disposition: 'attachment',
    is_text: false,
  },
  {
    index: HUGE_PART_INDEX,
    content_type: 'image/png',
    filename: 'huge.png',
    size: 50 * MIB,
    disposition: 'attachment',
    is_text: false,
  },
  {
    index: 6,
    content_type: 'text/html"><script>alert(1)</script>',
    filename: null,
    size: 10,
    disposition: 'attachment',
    is_text: false,
  },
  {
    index: FAKE_PNG_PART_INDEX,
    content_type: 'image/png',
    filename: 'photo.png',
    size: FAKE_IMAGE_BODY.length,
    disposition: 'attachment',
    is_text: false,
  },
  {
    index: BOMB_PART_INDEX,
    content_type: 'image/png',
    filename: 'bomb.png',
    size: BOMB_PNG_BODY.length,
    disposition: 'attachment',
    is_text: false,
  },
];

const HEADERS = [
  {
    name: 'Received',
    value: 'from mx.shop.example (mx.shop.example [192.0.2.10])\r\n by in.hooks.example with ESMTPS',
  },
  { name: 'From', value: 'Shop <no-reply@shop.example>' },
  { name: 'To', value: 'k7qm2xta9vnc@in.hooks.example' },
  { name: 'Reply-To', value: `support${RLO}@shop.example` },
  { name: 'Subject', value: HOSTILE_SUBJECT },
  { name: 'Authentication-Results', value: 'in.hooks.example; spf=pass; dkim=pass' },
  { name: 'X-Markup', value: '<script>alert(1)</script>' },
  { name: `X-${RLO}Name`, value: 'a'.repeat(5000) },
];

export const hostileEmailCapture = {
  ...jsonCaptureFixture,
  id: EMAIL_CAPTURE_ID,
  endpoint_id: ENDPOINT_ID,
  method: 'MAIL',
  path: '',
  query_string: '',
  query: [],
  form: null,
  forwards: [],
  headers: HEADERS,
  content_type: 'message/rfc822',
  body_size: encode(RAW_SOURCE).length,
  body: { encoding: 'utf8' as const, data: RAW_SOURCE, truncated: false },
  kind: 'email' as const,
  meta: {
    envelope_from: 'bounce@shop.example',
    envelope_to: ['k7qm2xta9vnc@in.hooks.example'],
    helo: 'mx.shop.example',
    tls: true,
    tag: null,
    subject: HOSTILE_SUBJECT,
    from: [`"Shop ${RLO}evil" <no-reply@shop.example>`],
    to: ['k7qm2xta9vnc@in.hooks.example'],
    cc: [],
    date: '2026-11-21T10:16:00.000Z',
    message_id: '<20261121.1@shop.example>',
    parts: PARTS,
    parse_error: null,
    parts_truncated: false,
  },
};

export function withRawMeta(meta: unknown): Capture {
  const next = { ...hostileEmailCapture, meta };
  return next;
}

export function withKind(kind: string): Capture {
  const next = { ...hostileEmailCapture, kind };
  return next;
}

export function withMeta(overrides: object): Capture {
  return withRawMeta({ ...hostileEmailCapture.meta, ...overrides });
}
