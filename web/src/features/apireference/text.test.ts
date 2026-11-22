import { describe, expect, it } from 'vitest';
import { plainText, textBlocks } from './text';

describe('textBlocks', () => {
  it('splits paragraphs and marks code spans without producing markup', () => {
    expect(textBlocks('`not_found`: gone\nsecond line\n\n<b>raw</b> `x`')).toEqual([
      [
        { text: 'not_found', code: true },
        { text: ': gone second line', code: false },
      ],
      [
        { text: '<b>raw</b> ', code: false },
        { text: 'x', code: true },
      ],
    ]);
  });

  it('keeps an unterminated backtick as literal text', () => {
    expect(textBlocks('a `b')).toEqual([
      [
        { text: 'a ', code: false },
        { text: '`b', code: false },
      ],
    ]);
    expect(plainText(null)).toBe('');
  });
});
