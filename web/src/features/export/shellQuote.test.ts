import { spawnSync } from 'node:child_process';
import process from 'node:process';
import { describe, expect, it } from 'vitest';
import { ExportError } from './errors';
import { shellQuote } from './shellQuote';

const CASES: ReadonlyArray<readonly [string, string, string]> = [
  ['single quote', "it's", "'it'\\''s'"],
  ['newline', 'a\nb', "'a\nb'"],
  ['carriage return', 'a\rb', `'a'"$(printf '\\015')"'b'`],
  ['tab', 'a\tb', "'a\tb'"],
  ['backslash', 'a\\b\\n', "'a\\b\\n'"],
  ['dollar', '$HOME $(id)', "'$HOME $(id)'"],
  ['backtick', '`id`', "'`id`'"],
  ['bang', 'a!b !!', "'a!b !!'"],
  ['unicode', 'héllo ✓ 日本', "'héllo ✓ 日本'"],
  ['escape sequence', 'a\u001b[201~b', `'a'"$(printf '\\033')"'[201~b'`],
  ['empty value', '', "''"],
];

describe('shellQuote', () => {
  it.each(CASES)('quotes %s', (_label, value, expected) => {
    expect(shellQuote(value)).toBe(expected);
  });

  it('refuses a NUL character', () => {
    expect(() => shellQuote('a\u0000b')).toThrow(ExportError);
  });

  it.skipIf(process.platform === 'win32').each(CASES)('survives a real shell for %s', (_label, value) => {
    const result = spawnSync('sh', ['-c', `printf %s ${shellQuote(value)}`]);
    expect(result.status).toBe(0);
    expect(new TextDecoder().decode(result.stdout)).toBe(value);
  });
});
