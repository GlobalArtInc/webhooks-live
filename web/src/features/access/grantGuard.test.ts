import { http } from 'msw';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { getEndpoint, setGrantRejectedHandler } from '../../api/client';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { errorResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { installGrantGuard, unlockUrl, type GuardRouter } from './grantGuard';
import { clearGrant, readGrant, storeGrant } from './grants';

registerMockServer();

const FUTURE = '2999-01-01T00:00:00.000Z';
const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;

interface SpyRouter extends GuardRouter {
  replace: ReturnType<typeof vi.fn<(path: string) => void>>;
}

afterEach(() => {
  clearGrant(ENDPOINT_ID);
  setGrantRejectedHandler(null);
});

function routerAt(pathname: string, status = 'idle'): SpyRouter {
  const replace = vi.fn<(path: string) => void>();
  return { state: { status, location: { pathname, href: pathname } }, history: { replace }, replace };
}

function answerWith(code: string): void {
  mockServer.use(http.get(`/api/v1/endpoints/${ENDPOINT_ID}`, () => errorResponse(401, code, 'Refused.')));
}

describe('grant guard', () => {
  it('builds an unlock address that carries the return path', () => {
    expect(unlockUrl(ENDPOINT_ID, '/e/x?order=asc')).toBe(`/e/${ENDPOINT_ID}/unlock?next=%2Fe%2Fx%3Forder%3Dasc`);
    expect(unlockUrl(ENDPOINT_ID, null)).toBe(`/e/${ENDPOINT_ID}/unlock`);
  });

  it('clears the dead grant and sends the visitor to the unlock page when a read answers grant_required', async () => {
    const router = routerAt(ENDPOINT_PATH);
    installGrantGuard(router);
    storeGrant(ENDPOINT_ID, 'fake-grant', FUTURE);
    answerWith('grant_required');

    await expect(getEndpoint(ENDPOINT_ID)).rejects.toMatchObject({ status: 401 });

    expect(readGrant(ENDPOINT_ID)).toBeNull();
    expect(router.replace).toHaveBeenCalledWith(unlockUrl(ENDPOINT_ID, ENDPOINT_PATH));
  });

  it('stays put while a load is pending or on the unlock page, and ignores other 401 codes', async () => {
    answerWith('grant_required');
    const pending = routerAt(ENDPOINT_PATH, 'pending');
    installGrantGuard(pending);
    await expect(getEndpoint(ENDPOINT_ID)).rejects.toBeDefined();
    expect(pending.replace).not.toHaveBeenCalled();

    const onUnlock = routerAt(`${ENDPOINT_PATH}/unlock`);
    installGrantGuard(onUnlock);
    await expect(getEndpoint(ENDPOINT_ID)).rejects.toBeDefined();
    expect(onUnlock.replace).not.toHaveBeenCalled();

    answerWith('unauthenticated');
    const other = routerAt(ENDPOINT_PATH);
    installGrantGuard(other);
    await expect(getEndpoint(ENDPOINT_ID)).rejects.toBeDefined();
    expect(other.replace).not.toHaveBeenCalled();
  });
});
