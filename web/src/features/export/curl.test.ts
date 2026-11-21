import { describe, expect, it } from 'vitest';
import { OMITTED_REQUEST_HEADERS } from './constants';
import { curlBodyFileName, curlNeedsBodyFile, curlSkippedHeaders, toCurl } from './curl';
import { ExportError } from './errors';
import { binaryBody, makeCapture, utf8Body } from './testFixtures';

describe('toCurl', () => {
  it('omits headers that curl sets itself or that corrupt the replay', () => {
    const headers = [
      ...OMITTED_REQUEST_HEADERS.map((name) => ({ name: name.toUpperCase(), value: 'x' })),
      { name: 'X-Kept', value: 'yes' },
    ];
    const command = toCurl(makeCapture({ headers }));
    expect(command).toContain("-H 'X-Kept: yes'");
    for (const name of OMITTED_REQUEST_HEADERS) {
      expect(command.toLowerCase()).not.toContain(`-h '${name}:`);
    }
    expect(curlSkippedHeaders(makeCapture({ headers })).filter((h) => h.reason === 'omitted')).toHaveLength(
      OMITTED_REQUEST_HEADERS.length,
    );
  });

  it('needs no -X for GET without a body and always passes -X otherwise', () => {
    expect(toCurl(makeCapture({ method: 'GET' }))).not.toContain('-X');
    expect(toCurl(makeCapture({ method: 'DELETE' }))).toContain("-X 'DELETE'");
    expect(toCurl(makeCapture({ method: 'GET', body: utf8Body('a') }))).toContain("-X 'GET'");
  });

  it('repeats multi-valued headers in original order', () => {
    const headers = [
      { name: 'X-Tag', value: 'one' },
      { name: 'X-Other', value: 'mid' },
      { name: 'X-Tag', value: 'two' },
    ];
    const command = toCurl(makeCapture({ headers }));
    expect(command.indexOf("'X-Tag: one'")).toBeLessThan(command.indexOf("'X-Other: mid'"));
    expect(command.indexOf("'X-Other: mid'")).toBeLessThan(command.indexOf("'X-Tag: two'"));
  });

  it('does not emit a header whose value contains a newline and reports it', () => {
    const headers = [
      { name: 'X-Evil', value: 'a\nb: c' },
      { name: 'X-Ok', value: 'fine' },
    ];
    const capture = makeCapture({ headers });
    const command = toCurl(capture);
    expect(command).not.toContain('X-Evil');
    expect(command).toContain("'X-Ok: fine'");
    expect(curlSkippedHeaders(capture)).toEqual([{ name: 'X-Evil', value: 'a\nb: c', reason: 'unsendable' }]);
  });

  it('sends a text body as a quoted --data-raw literal', () => {
    const command = toCurl(makeCapture({ body: utf8Body("it's") }));
    expect(command).toContain("--data-raw 'it'\\''s'");
    expect(command).not.toContain('printf');
  });

  it('sends a binary body through printf and --data-binary @-', () => {
    const command = toCurl(makeCapture({ body: binaryBody([0, 255, 0x27]) }));
    expect(command.startsWith("printf '\\000\\377\\047' | curl ")).toBe(true);
    expect(command).toContain('--data-binary @-');
    expect(command).not.toContain('--data-raw');
  });

  it('produces backslash continuations in multi-line form', () => {
    const command = toCurl(makeCapture({ headers: [{ name: 'X-A', value: '1' }] }), { multiline: true });
    expect(
      command
        .split('\n')
        .slice(0, -1)
        .every((line) => line.endsWith(' \\')),
    ).toBe(true);
    expect(command.split('\n').length).toBeGreaterThan(3);
  });

  it('refuses a truncated body', () => {
    const body = { encoding: 'utf8' as const, data: 'abc', truncated: true };
    expect(() => toCurl(makeCapture({ body }))).toThrow(ExportError);
  });

  it('keeps a tab literal in a text body', () => {
    const command = toCurl(makeCapture({ body: utf8Body('a\tb') }));
    expect(command).toContain("--data-raw 'a\tb'");
  });

  it('sends a text body with control characters through the printf pipe', () => {
    const command = toCurl(makeCapture({ body: utf8Body('a\r\nb\u001b') }));
    expect(command.startsWith("printf 'a\\015\\012b\\033' | curl ")).toBe(true);
    expect(command).toContain('--data-binary @-');
    expect(command).not.toContain('--data-raw');
    expect(command).not.toContain('$(');
  });

  it('escapes a leading dash so printf does not read the body as an option', () => {
    const command = toCurl(makeCapture({ body: binaryBody([0x2d, 0x2d, 0x62, 0]) }));
    expect(command.startsWith("printf '\\055-b\\000' | curl ")).toBe(true);
  });

  it('sends a text body above the pipe threshold through the printf pipe', () => {
    const capture = makeCapture({ body: utf8Body('x'.repeat(11)) });
    expect(toCurl(capture, { largeBodyPipeThreshold: 10 })).toMatch(/^printf 'x{11}' \| curl /);
    expect(toCurl(capture, { largeBodyPipeThreshold: 11 })).toContain('--data-raw');
  });

  it('refers to a body file above the file threshold and does not inline the body', () => {
    const capture = makeCapture({ body: utf8Body('secret-body-text') });
    const options = { largeBodyPipeThreshold: 4, largeBodyFileThreshold: 10 };
    const command = toCurl(capture, options);
    expect(command).toContain("--data-binary '@capture-0193a1c2.bin'");
    expect(command).not.toContain('secret-body-text');
    expect(command).not.toContain('printf');
    expect(curlBodyFileName(capture)).toBe('capture-0193a1c2.bin');
    expect(curlNeedsBodyFile(capture, options)).toBe(true);
    expect(curlNeedsBodyFile(capture)).toBe(false);
  });

  it('uses the default thresholds of 64 KiB and 256 KiB', () => {
    const at = (size: number) => makeCapture({ body: utf8Body('x'.repeat(size)) });
    expect(toCurl(at(64 * 1024))).toContain('--data-raw');
    expect(toCurl(at(64 * 1024 + 1))).toContain('--data-binary @-');
    expect(toCurl(at(256 * 1024 + 1))).toContain("--data-binary '@capture-");
  });

  it('rejects an invalid threshold', () => {
    expect(() => toCurl(makeCapture(), { largeBodyPipeThreshold: -1 })).toThrow(ExportError);
  });

  it('sends Content-Length 0 for an empty POST, PUT or PATCH body only', () => {
    for (const method of ['POST', 'PUT', 'PATCH']) {
      expect(toCurl(makeCapture({ method }))).toContain("--data-raw ''");
    }
    expect(toCurl(makeCapture({ method: 'DELETE' }))).not.toContain('--data-raw');
    expect(toCurl(makeCapture({ method: 'GET' }))).not.toContain('--data-raw');
  });

  it('uses --head for HEAD instead of -X HEAD', () => {
    const command = toCurl(makeCapture({ method: 'HEAD' }));
    expect(command).toContain('--head');
    expect(command).not.toContain('-X');
  });

  it('trims only space and tab, so a non-breaking space is a real value', () => {
    const headers = [
      { name: 'X-Nbsp', value: ' ' },
      { name: 'X-Blank', value: ' \t ' },
    ];
    const command = toCurl(makeCapture({ headers }));
    expect(command).toContain("-H 'X-Nbsp:  '");
    expect(command).toContain("-H 'X-Blank;'");
  });

  it('rejects a method that is not an HTTP token', () => {
    for (const method of ['', 'GE T', 'GET\n', 'GE(T)', 'A:B']) {
      expect(() => toCurl(makeCapture({ method }))).toThrow(ExportError);
    }
  });

  it('skips headers with an invalid name and reports them', () => {
    const headers = [
      { name: 'Bad Name', value: '1' },
      { name: 'Bad:Name', value: '2' },
      { name: '', value: '3' },
    ];
    const capture = makeCapture({ method: 'GET', headers });
    expect(toCurl(capture)).not.toContain('-H');
    expect(curlSkippedHeaders(capture).map((h) => h.reason)).toEqual(['unsendable', 'unsendable', 'unsendable']);
  });

  it('escapes a few control characters in a header value and skips a long run of them', () => {
    const few = makeCapture({ headers: [{ name: 'X-Few', value: 'a\u001bb\u0003' }] });
    expect(toCurl(few)).toContain(`-H 'X-Few: a'"$(printf '\\033')"'b'"$(printf '\\003')"`);
    const many = makeCapture({ headers: [{ name: 'X-Many', value: '\u0001a'.repeat(9) }] });
    expect(toCurl(many)).not.toContain('X-Many');
    expect(curlSkippedHeaders(many)).toEqual([{ name: 'X-Many', value: '\u0001a'.repeat(9), reason: 'unsendable' }]);
  });

  it('refuses a URL with many control characters', () => {
    expect(() => toCurl(makeCapture({ url: `https://h/${'\u0001a'.repeat(9)}` }))).toThrow(ExportError);
  });
});
