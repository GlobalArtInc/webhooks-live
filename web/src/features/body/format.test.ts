import { describe, expect, it } from 'vitest';
import { formatHex } from './bytes';
import { formatJson, formatXml } from './format';
import { BINARY_BODY, INVALID_JSON_BODY, JSON_BODY, XML_BODY } from './fixtures';

describe('formatJson', () => {
  it('indents with two spaces and keeps number and string literals verbatim', () => {
    const result = formatJson(
      '{"big":12345678901234567890,"price":1.50,"s":"a\\u00e9\\n","list":[1,2],"empty":{},"none":[]}',
    );
    expect(result).toEqual({
      ok: true,
      text: [
        '{',
        '  "big": 12345678901234567890,',
        '  "price": 1.50,',
        '  "s": "a\\u00e9\\n",',
        '  "list": [',
        '    1,',
        '    2',
        '  ],',
        '  "empty": {},',
        '  "none": []',
        '}',
      ].join('\n'),
    });
  });

  it('formats the realistic fixture', () => {
    const result = formatJson(JSON_BODY);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.text.split('\n')).toContain('    "object": {');
    }
  });

  it('reports the line and column of a syntax error', () => {
    const result = formatJson(INVALID_JSON_BODY);
    expect(result).toEqual({
      ok: false,
      error: { message: 'Not valid JSON at 4:13', line: 4, column: 13 },
    });
  });

  it('reports an error at the end of truncated input', () => {
    expect(formatJson('{"a":')).toEqual({
      ok: false,
      error: { message: 'Not valid JSON at 1:6', line: 1, column: 6 },
    });
  });

  it('rejects trailing content after the document', () => {
    expect(formatJson('{} x')).toMatchObject({ ok: false, error: { line: 1, column: 4 } });
  });

  it('refuses nesting beyond the depth limit instead of recursing', () => {
    const result = formatJson('['.repeat(5000));
    expect(result).toMatchObject({ ok: false });
  });

  it('stops when the formatted output would explode', () => {
    const deep = `${'['.repeat(120)}${'1,'.repeat(40000)}1${']'.repeat(120)}`;
    expect(formatJson(deep)).toMatchObject({ ok: false });
  });
});

describe('formatXml', () => {
  it('indents nested elements and keeps text-only elements on one line', () => {
    const result = formatXml(XML_BODY);
    expect(result).toEqual({
      ok: true,
      text: [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<order id="6735">',
        '  <customer>',
        '    <name>Ada Lovelace</name>',
        '    <email>ada@example.com</email>',
        '  </customer>',
        '  <items>',
        '    <item sku="A-1" qty="2"/>',
        '    <item sku="B-7" qty="1">',
        '      <note>Gift wrap</note>',
        '    </item>',
        '  </items>',
        '  <!-- paid -->',
        '</order>',
      ].join('\n'),
    });
  });

  it('reports a mismatched closing tag with its position', () => {
    expect(formatXml('<a>\n  <b></a>\n</a>')).toEqual({
      ok: false,
      error: { message: 'Not valid XML at 2:6', line: 2, column: 6 },
    });
  });
});

describe('formatHex', () => {
  it('prints offset, hex pairs and an ASCII column', () => {
    expect(formatHex(BINARY_BODY.subarray(0, 12))).toBe(
      '00000000  00 01 02 03 ff fe fd 41  42 43 0a 00              |.......ABC..|',
    );
  });

  it('stops at the limit', () => {
    expect(formatHex(new Uint8Array(100), 16).split('\n')).toHaveLength(1);
  });
});
