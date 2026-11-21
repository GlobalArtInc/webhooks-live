import { setupServer } from 'msw/node';
import { afterAll, afterEach, beforeAll } from 'vitest';
import { handlers } from './handlers';
import { organizeHandlers } from './organize';
import { resetMockStore } from './store';

export const mockServer = setupServer(...organizeHandlers, ...handlers);

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
