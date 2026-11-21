import { execFile, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { IncomingMessage, Server, ServerResponse } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import process from 'node:process';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { BODY_METHODS, OMITTED_REQUEST_HEADERS } from './constants';
import { curlBodyFileName, curlNeedsBodyFile, toCurl } from './curl';
import { binaryBody, makeCapture, utf8Body } from './testFixtures';
import type { CaptureBody, CurlOptions, NameValue } from './types';

const MARKER = 'SHOULD_NOT_EXIST';
const INJECTION = "'; touch SHOULD_NOT_EXIST; echo '";
const SHELLS: readonly string[] = ['sh', 'dash', 'bash'];
const curlAvailable = spawnSync('curl', ['--version']).status === 0;
const ciValue = process.env['CI'];
const ciEnabled = ciValue !== undefined && ciValue !== '' && ciValue !== '0' && ciValue !== 'false';

function shellAvailable(shell: string): boolean {
  return spawnSync(shell, ['-c', 'true']).status === 0;
}

function multipartLines(count: number): string {
  const lines: string[] = [];
  for (let i = 0; i < count; i += 1) {
    lines.push(i % 3 === 0 ? '--b' : i % 3 === 1 ? `Content-Disposition: form-data; name="f${i}"` : `v ${i} it's`);
  }
  return lines.join('\r\n');
}

function repeatedText(size: number): string {
  const line = "it's a line with 'quotes' and $(touch SHOULD_NOT_EXIST) and `ticks`\n";
  return line.repeat(Math.ceil(size / line.length)).slice(0, size);
}

interface Received {
  method: string;
  url: string;
  headers: NameValue[];
  body: Uint8Array;
}

interface ReceiveState {
  received: Received | null;
}

interface RoundTripCase {
  label: string;
  method: string;
  path: string;
  headers: NameValue[];
  body: CaptureBody;
  options?: CurlOptions;
}

const CASES: RoundTripCase[] = [
  {
    label: 'a body with single quotes, newlines and CRLF',
    method: 'POST',
    path: '/a/b?x[1]=2&y={z}&q=it%27s',
    headers: [
      { name: 'Content-Type', value: 'text/plain; charset=utf-8' },
      { name: 'X-Quote', value: 'it\'s "quoted" \\ back' },
    ],
    body: utf8Body("it's a 'test'\nline two\r\nline three\n\n"),
  },
  {
    label: 'command substitution and backticks in the body and a header',
    method: 'PUT',
    path: '/inject',
    headers: [
      { name: 'Content-Type', value: 'application/json' },
      { name: 'X-Evil', value: `$(touch ${MARKER}) \`touch ${MARKER}\` ${INJECTION} $HOME ! #` },
    ],
    body: utf8Body(`{"a":"$(touch ${MARKER})","b":"\`touch ${MARKER}\`","c":"${INJECTION}"}`),
  },
  {
    label: 'a binary body with NUL and 0xff bytes and no content type',
    method: 'POST',
    path: '/bin',
    headers: [{ name: 'X-Kind', value: 'binary' }],
    body: binaryBody([0, 255, 254, 10, 13, 0x25, 0x5c, 0x27, 0x30, 0, 0x31, 0x80, 0xc3]),
  },
  {
    label: 'unicode, a BOM, tabs and terminal escape bytes in a text body',
    method: 'PATCH',
    path: '/uni',
    headers: [
      { name: 'Content-Type', value: 'text/plain' },
      { name: 'X-Unicode', value: 'héllo ✓ 日本' },
    ],
    body: utf8Body('﻿col1\tcol2\n\u001b[201~ echo pwned\r\u0003\u007f\u0085 end ✓'),
  },
  {
    label: 'multi-valued and empty headers with omitted ones present',
    method: 'GET',
    path: '/headers',
    headers: [
      { name: 'Host', value: 'hooks.example' },
      { name: 'Content-Length', value: '999' },
      { name: 'Connection', value: 'close' },
      { name: 'X-Tag', value: 'one' },
      { name: 'X-Empty', value: '' },
      { name: 'X-Tag', value: 'two' },
    ],
    body: utf8Body(''),
  },
  {
    label: 'the multi-line form',
    method: 'POST',
    path: '/multi',
    headers: [{ name: 'Content-Type', value: 'text/plain' }],
    body: utf8Body("a'b\n$(id)"),
    options: { multiline: true },
  },
  {
    label: 'an empty POST body with Content-Length 0',
    method: 'POST',
    path: '/empty',
    headers: [{ name: 'X-Kind', value: 'empty' }],
    body: utf8Body(''),
  },
  {
    label: 'a 70 KiB text body through the size-triggered printf pipe',
    method: 'POST',
    path: '/pipe',
    headers: [{ name: 'Content-Type', value: 'text/plain' }],
    body: utf8Body(repeatedText(70 * 1024)),
  },
  {
    label: 'a 1 MiB text body through the body file tier',
    method: 'POST',
    path: '/file',
    headers: [{ name: 'Content-Type', value: 'text/plain' }],
    body: utf8Body(repeatedText(1024 * 1024)),
  },
];

const MULTIPART_CASE: RoundTripCase = {
  label: 'a 3000-line CRLF multipart text body',
  method: 'POST',
  path: '/multipart',
  headers: [{ name: 'Content-Type', value: 'multipart/form-data; boundary=b' }],
  body: utf8Body(multipartLines(3000)),
};

function resetReceived(target: ReceiveState): void {
  target.received = null;
}

function latin1ToUtf8(raw: string): string {
  return new TextDecoder().decode(Uint8Array.from(raw, (char) => char.charCodeAt(0)));
}

async function readBody(req: IncomingMessage): Promise<Uint8Array> {
  const chunks: Uint8Array[] = [];
  let total = 0;
  for await (const chunk of req) {
    chunks.push(chunk);
    total += chunk.length;
  }
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.length;
  }
  return out;
}

function expectedBytes(body: CaptureBody): Uint8Array {
  return body.encoding === 'utf8'
    ? new TextEncoder().encode(body.data)
    : Uint8Array.from(atob(body.data), (char) => char.charCodeAt(0));
}

function runShell(shell: string, command: string, cwd: string): Promise<void> {
  return new Promise((resolve, reject) => {
    execFile(
      shell,
      ['-c', command],
      { cwd, timeout: 15000, env: { PATH: process.env['PATH'] ?? '', HOME: cwd } },
      (error) => (error === null ? resolve() : reject(error)),
    );
  });
}

describe('curl availability', () => {
  it('is on PATH whenever CI is set', () => {
    expect(!ciEnabled || curlAvailable).toBe(true);
  });
});

describe.skipIf(!curlAvailable || process.platform === 'win32')('toCurl round trip through sh and curl', () => {
  const state: ReceiveState = { received: null };
  let server: Server;
  let port = 0;
  let workDir = '';

  async function handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const body = await readBody(req);
    const headers: NameValue[] = [];
    for (let i = 0; i < req.rawHeaders.length; i += 2) {
      headers.push({ name: req.rawHeaders[i] ?? '', value: latin1ToUtf8(req.rawHeaders[i + 1] ?? '') });
    }
    state.received = { method: req.method ?? '', url: req.url ?? '', headers, body };
    res.statusCode = 204;
    res.end();
  }

  beforeAll(async () => {
    workDir = mkdtempSync(join(tmpdir(), 'whl-curl-'));
    server = createServer((req, res) => {
      handle(req, res).catch(() => {
        res.statusCode = 500;
        res.end();
      });
    });
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
    const address = server.address();
    if (address === null || typeof address === 'string') {
      throw new Error('test server has no port');
    }
    port = address.port;
  });

  afterAll(async () => {
    await new Promise<void>((resolve) => server.close(resolve));
    rmSync(workDir, { recursive: true, force: true });
  });

  async function replay(testCase: RoundTripCase, shell: string): Promise<void> {
    const capture = makeCapture({
      method: testCase.method,
      url: `http://127.0.0.1:${port}${testCase.path}`,
      headers: testCase.headers,
      body: testCase.body,
    });
    const options = testCase.options ?? {};
    const bytes = expectedBytes(testCase.body);
    if (curlNeedsBodyFile(capture, options)) {
      writeFileSync(join(workDir, curlBodyFileName(capture)), bytes);
    }
    resetReceived(state);

    await runShell(shell, toCurl(capture, options), workDir);

    expect(existsSync(join(workDir, MARKER))).toBe(false);
    const received = state.received;
    expect(received).not.toBeNull();
    if (received === null) {
      return;
    }
    expect(received.method).toBe(testCase.method);
    expect(received.url).toBe(testCase.path);
    expect(received.body).toEqual(bytes);

    const sent = testCase.headers.filter((h) => !OMITTED_REQUEST_HEADERS.includes(h.name.toLowerCase()));
    const watched = new Set([...sent.map((h) => h.name.toLowerCase()), 'content-type']);
    expect(received.headers.filter((h) => watched.has(h.name.toLowerCase()))).toEqual(sent);
    const hostHeader = received.headers.find((h) => h.name.toLowerCase() === 'host');
    expect(hostHeader?.value).toBe(`127.0.0.1:${port}`);
    expect(received.headers.some((h) => h.name.toLowerCase() === 'connection')).toBe(false);
    const lengthHeader = received.headers.find((h) => h.name.toLowerCase() === 'content-length');
    const expectsLength = bytes.length > 0 || BODY_METHODS.includes(testCase.method);
    expect(lengthHeader?.value).toBe(expectsLength ? String(bytes.length) : undefined);
  }

  it.each(CASES)('reproduces $label byte for byte', async (testCase) => {
    await replay(testCase, 'sh');
  });

  for (const shell of SHELLS) {
    it.skipIf(!shellAvailable(shell))(`reproduces a 3000-line CRLF multipart text body through ${shell}`, async () => {
      await replay(MULTIPART_CASE, shell);
    });
  }
});
