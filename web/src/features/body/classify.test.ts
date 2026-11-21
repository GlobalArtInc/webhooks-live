import { describe, expect, it } from 'vitest';
import { classifyBody } from './classify';
import {
  BINARY_BODY,
  encode,
  generateLargeBody,
  HTML_BODY,
  imageHeader,
  INVALID_JSON_BODY,
  INVALID_UTF8_BODY,
  JSON_BODY,
  MIB,
  PNG_1X1,
  SVG_BODY,
  TEXT_BODY,
  XML_BODY,
} from './fixtures';
import type { ImageType } from './types';

describe('classifyBody', () => {
  it('classifies a JSON content type as json', () => {
    expect(classifyBody(encode(JSON_BODY), 'application/json; charset=utf-8')).toEqual({ kind: 'json' });
  });

  it('classifies a +json content type as json', () => {
    expect(classifyBody(encode(JSON_BODY), 'application/vnd.api+json')).toEqual({ kind: 'json' });
  });

  it('sniffs a leading brace that parses as json under a text content type', () => {
    expect(classifyBody(encode(JSON_BODY), 'text/plain')).toEqual({ kind: 'json' });
  });

  it('keeps a leading brace that does not parse as text when the content type is not json', () => {
    expect(classifyBody(encode(INVALID_JSON_BODY), 'text/plain')).toEqual({ kind: 'text' });
  });

  it('keeps invalid json as json when the content type says json', () => {
    expect(classifyBody(encode(INVALID_JSON_BODY), 'application/json')).toEqual({ kind: 'json' });
  });

  it('classifies xml and +xml content types as xml', () => {
    expect(classifyBody(encode(XML_BODY), 'application/xml')).toEqual({ kind: 'xml' });
    expect(classifyBody(encode(XML_BODY), 'application/soap+xml')).toEqual({ kind: 'xml' });
  });

  it('classifies a plain text body as text', () => {
    expect(classifyBody(encode(TEXT_BODY), 'text/plain')).toEqual({ kind: 'text' });
    expect(classifyBody(encode(TEXT_BODY), '')).toEqual({ kind: 'text' });
  });

  it('classifies text/html as html', () => {
    expect(classifyBody(encode(HTML_BODY), 'text/html; charset=utf-8')).toEqual({ kind: 'html' });
  });

  it('classifies a body with a NUL byte as binary', () => {
    expect(classifyBody(BINARY_BODY, 'application/octet-stream')).toEqual({ kind: 'binary' });
  });

  it('classifies a body with invalid UTF-8 as binary', () => {
    expect(classifyBody(INVALID_UTF8_BODY, 'text/plain')).toEqual({ kind: 'binary' });
  });

  it('does not treat a multi-byte character cut at the sniff boundary as invalid', () => {
    const body = encode(`${'a'.repeat(8191)}é${'b'.repeat(100)}`);
    expect(classifyBody(body, 'text/plain')).toEqual({ kind: 'text' });
  });

  it.each<ImageType>(['png', 'jpeg', 'gif', 'webp'])('classifies %s magic bytes as an image', (image) => {
    expect(classifyBody(imageHeader(image), 'application/octet-stream')).toEqual({ kind: 'image', image });
  });

  it('decides images by magic bytes and ignores a text/html content type', () => {
    expect(classifyBody(PNG_1X1, 'text/html')).toEqual({ kind: 'image', image: 'png' });
  });

  it('classifies svg as text, never as an image', () => {
    expect(classifyBody(encode(SVG_BODY), 'image/svg+xml')).toEqual({ kind: 'text' });
  });

  it('classifies a body over 1 MiB as large and keeps a hint without parsing it', () => {
    const body = generateLargeBody(MIB + 1);
    expect(classifyBody(body, 'application/json')).toEqual({ kind: 'large', hint: { kind: 'json' } });
  });

  it('classifies by the announced total size when only a prefix is held', () => {
    const prefix = generateLargeBody(1024).subarray(0, 1024);
    expect(classifyBody(prefix, 'text/plain', 4 * MIB)).toEqual({ kind: 'large', hint: { kind: 'json' } });
  });
});
