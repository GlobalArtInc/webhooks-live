import { describe, expect, it } from 'vitest';
import { clampText, escapeInert, escapeMultiline, segmentInert, truncateText, unfoldHeader } from './inert';

describe('escapeInert', () => {
  it('leaves ordinary text and emoji joiners alone', () => {
    expect(escapeInert('Invoice 2026-11 \u00E9')).toBe('Invoice 2026-11 \u00E9');
    expect(escapeInert('\u{1F469}\u200D\u{1F4BB}')).toBe('\u{1F469}\u200D\u{1F4BB}');
  });

  it('writes bidi controls and invisible characters as visible escapes', () => {
    expect(escapeInert('a\u202Eb\u2069c')).toBe('a\\u{202E}b\\u{2069}c');
    expect(escapeInert('x\u200By\uFEFFz')).toBe('x\\u{200B}y\\u{FEFF}z');
  });

  it('writes invisible letters and every space except the plain one as visible escapes', () => {
    expect(escapeInert('invoice.pdf\u3164.exe')).toBe('invoice.pdf\\u{3164}.exe');
    expect(escapeInert('a\u2800b\u115Fc\u1160d')).toBe('a\\u{2800}b\\u{115F}c\\u{1160}d');
    expect(escapeInert('a b\u00A0c\u3000d')).toBe('a b\\u{00A0}c\\u{3000}d');
  });

  it('writes control characters, line breaks and lone surrogates as visible escapes', () => {
    expect(escapeInert('a\u0000b\u001Bc\u0085d')).toBe('a\\u{0000}b\\u{001B}c\\u{0085}d');
    expect(escapeInert('a\r\nb\tc')).toBe('a\\r\\nb\\tc');
    expect(escapeInert('a\uD800b')).toBe('a\\u{D800}b');
  });
});

describe('escapeMultiline', () => {
  it('keeps line feeds and tabs, folds CRLF and escapes a lone carriage return', () => {
    expect(escapeMultiline('a\r\nb\tc\rd')).toBe('a\nb\tc\\rd');
  });

  it('escapes bidi controls inside a long text', () => {
    expect(escapeMultiline('line\n\u202Eevil\n')).toBe('line\n\\u{202E}evil\n');
  });
});

describe('segmentInert', () => {
  it('splits text from escapes in order', () => {
    expect(segmentInert('ab\u202Ecd')).toEqual([
      { text: 'ab', escaped: false },
      { text: '\\u{202E}', escaped: true },
      { text: 'cd', escaped: false },
    ]);
  });

  it('returns nothing for an empty string', () => {
    expect(segmentInert('')).toEqual([]);
  });
});

describe('clampText and unfoldHeader', () => {
  it('cuts long strings without leaving half a surrogate pair', () => {
    expect(clampText('abcdef', 3)).toBe('abc\u2026');
    expect(clampText('ab\u{1F600}', 3)).toBe('ab\u2026');
    expect(clampText('abc', 3)).toBe('abc');
    expect(truncateText('ab\u{1F600}', 3)).toBe('ab');
    expect(truncateText('abc', 0)).toBe('');
  });

  it('unfolds continuation lines into one line', () => {
    expect(unfoldHeader('one\r\n two\n\tthree')).toBe('one two three');
  });
});
