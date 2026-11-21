import { setupServer } from 'msw/node';
import { afterAll, afterEach, beforeAll } from 'vitest';
import { generalHandlers } from './general';
import { handlers } from './handlers';
import { organizeHandlers } from './organize';
import { resetMockStore } from './store';
import { resetUsageStore, usageHandlers } from './usage';

export const mockServer = setupServer(...organizeHandlers, ...handlers, ...generalHandlers, ...usageHandlers);

export function registerMockServer(): void {
  beforeAll(() => {
    mockServer.listen({ onUnhandledRequest: 'error' });
  });
  afterEach(() => {
    mockServer.resetHandlers();
    resetMockStore();
    resetUsageStore();
  });
  afterAll(() => {
    mockServer.close();
  });
}
