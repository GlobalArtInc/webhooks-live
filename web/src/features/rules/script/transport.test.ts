import { setupServer } from 'msw/node';
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../../../api/client';
import { SCRIPT_META_FIXTURE } from './fixtures';
import { SCRIPT_META_URL, SCRIPT_VALIDATE_URL, scriptHandlers } from './scriptMocks';
import { httpScriptTransport } from './transport';

function stubFetch(response: Response) {
  const fetchMock = vi.fn(() => Promise.resolve(response));
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('httpScriptTransport', () => {
  it('reads the language meta with a GET that carries the abort signal', async () => {
    const fetchMock = stubFetch(Response.json(SCRIPT_META_FIXTURE));
    const controller = new AbortController();

    await expect(httpScriptTransport.loadMeta(controller.signal)).resolves.toEqual(SCRIPT_META_FIXTURE);

    const [url, init] = fetchMock.mock.calls[0] as unknown as [URL, RequestInit];
    expect(url.pathname).toBe(SCRIPT_META_URL);
    expect(init.method).toBe('GET');
    expect(init.signal).toBe(controller.signal);
  });

  it('posts the source as JSON and returns parsed diagnostics', async () => {
    const answer = {
      diagnostics: [{ line: 1, column: 2, length: 3, code: 'script_invalid', message: 'Bad', severity: 'error' }],
    };
    const fetchMock = stubFetch(Response.json(answer));
    const controller = new AbortController();

    const result = await httpScriptTransport.validate('let a = ;', controller.signal);

    expect(result).toEqual(answer.diagnostics);
    const [url, init] = fetchMock.mock.calls[0] as unknown as [URL, RequestInit];
    expect(url.pathname).toBe(SCRIPT_VALIDATE_URL);
    expect(init.method).toBe('POST');
    expect(init.body).toBe(JSON.stringify({ source: 'let a = ;' }));
    expect(init.signal).toBe(controller.signal);
  });

  it('turns an error answer into an ApiError with its status and code', async () => {
    stubFetch(Response.json({ error: { code: 'feature_disabled', message: 'Scripts are off' } }, { status: 503 }));

    const failure = await httpScriptTransport.loadMeta(new AbortController().signal).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toMatchObject({ status: 503, code: 'feature_disabled', message: 'Scripts are off' });
  });

  it('refuses an answer that breaks the contract', async () => {
    stubFetch(Response.json({ diagnostics: [{ line: 0 }] }));

    await expect(httpScriptTransport.validate('x', new AbortController().signal)).rejects.toThrow(/diagnostics\[0\]/);
  });
});

describe('script mock handlers', () => {
  const server = setupServer(...scriptHandlers);

  beforeAll(() => {
    server.listen({ onUnhandledRequest: 'error' });
  });

  afterAll(() => {
    server.close();
  });

  it('serve the meta and validate a source like the server fixture does', async () => {
    const origin = window.location.origin;
    const meta = await fetch(`${origin}${SCRIPT_META_URL}`).then((response) => response.json());
    expect(meta).toEqual(SCRIPT_META_FIXTURE);

    const validated = await fetch(`${origin}${SCRIPT_VALIDATE_URL}`, {
      method: 'POST',
      body: JSON.stringify({ source: 'let a = 1 +' }),
    }).then((response) => response.json());
    expect(validated.diagnostics[0]).toMatchObject({ line: 1, code: 'script_invalid' });

    const refused = await fetch(`${origin}${SCRIPT_VALIDATE_URL}`, { method: 'POST', body: JSON.stringify({}) });
    expect(refused.status).toBe(400);
  });
});
