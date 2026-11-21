import type { Bytes, ImageType } from './types';

export const MIB = 1024 * 1024;

export function encode(text: string): Bytes {
  return new TextEncoder().encode(text);
}

export const JSON_BODY = `{"id":"evt_1QZk2eLkdIwHu7ix","object":"event","type":"charge.succeeded","livemode":false,"data":{"object":{"amount":2000,"currency":"usd","paid":true,"metadata":{"order_id":"6735","note":"line one\\nline two"},"refunds":[]}}}`;

export const INVALID_JSON_BODY = `{
  "id": "evt_1QZk2eLkdIwHu7ix",
  "amount": 2000,
  "status": ,
  "currency": "usd"
}`;

export const XML_BODY = `<?xml version="1.0" encoding="UTF-8"?><order id="6735"><customer><name>Ada Lovelace</name><email>ada@example.com</email></customer><items><item sku="A-1" qty="2"/><item sku="B-7" qty="1"><note>Gift wrap</note></item></items><!-- paid --></order>`;

export const TEXT_BODY = `2026-11-21T10:02:11Z deploy finished
service=api version=1.42.0 status=ok
`;

export const HTML_BODY = `<!doctype html><html><head><title>Hello</title></head><body><script>document.title = 'pwned';</script><img src="x" onerror="alert(1)"><a href="https://example.com/a">https://example.com/b</a></body></html>`;

export const SVG_BODY = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><script>alert(1)</script><circle cx="5" cy="5" r="4"/></svg>`;

export const BINARY_BODY: Bytes = Uint8Array.from([
  0x00, 0x01, 0x02, 0x03, 0xff, 0xfe, 0xfd, 0x41, 0x42, 0x43, 0x0a, 0x00, 0x7f, 0x80, 0x81, 0x82, 0x52, 0x49, 0x46,
  0x58,
]);

export const INVALID_UTF8_BODY: Bytes = Uint8Array.from([0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0xc3, 0x28, 0x20, 0x77]);

const PNG_BASE64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';

function decodeBase64(value: string): Bytes {
  return Uint8Array.from(atob(value), (char) => char.charCodeAt(0));
}

export const PNG_1X1: Bytes = decodeBase64(PNG_BASE64);

const HEADERS: Readonly<Record<ImageType, readonly number[]>> = {
  png: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d],
  jpeg: [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01],
  gif: [0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00],
  webp: [0x52, 0x49, 0x46, 0x46, 0x24, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x20],
};

export function imageHeader(image: ImageType): Bytes {
  return Uint8Array.from(HEADERS[image]);
}

export function generateLargeBody(minimumSize: number): Bytes {
  const entries: string[] = [];
  let length = 2;
  for (let index = 0; length < minimumSize; index += 1) {
    const entry = `{"index":${index},"event":"order.updated","payload":"${'x'.repeat(96)}"}`;
    entries.push(entry);
    length += entry.length + 2;
  }
  return encode(`[${entries.join(',\n')}]`);
}
