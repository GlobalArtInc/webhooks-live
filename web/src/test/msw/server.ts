import { setupServer } from 'msw/node';
import { afterAll, afterEach, beforeAll } from 'vitest';
import { handlers } from './handlers';
import { resetMockStore } from './store';

export const mockServer = setupServer(...handlers);

export function registerMockServer(): void {
  beforeAll(() => {
    mockServer.listen({ onUnhandledRequest: 'error' });
  });
  afterEach(() => {
    mockServer.resetHandlers();
    resetMockStore();
  });
  afterAll(() => {
    mockServer.close();
  });
}
