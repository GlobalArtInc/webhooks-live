import { setupServer } from 'msw/node';
import { afterAll, afterEach, beforeAll } from 'vitest';
import { generalHandlers } from './general';
import { handlers } from './handlers';
import { openApiHandlers, resetOpenApiStore } from './openapi';
import { organizeHandlers } from './organize';
import { resetRuleRunStore, ruleRunHandlers } from './runLogs';
import { resetMockStore } from './store';
import { resetUsageStore, usageHandlers } from './usage';

export const mockServer = setupServer(
  ...ruleRunHandlers,
  ...organizeHandlers,
  ...handlers,
  ...generalHandlers,
  ...usageHandlers,
  ...openApiHandlers,
);

export function registerMockServer(): void {
  beforeAll(() => {
    mockServer.listen({ onUnhandledRequest: 'error' });
  });
  afterEach(() => {
    mockServer.resetHandlers();
    resetMockStore();
    resetUsageStore();
    resetOpenApiStore();
    resetRuleRunStore();
  });
  afterAll(() => {
    mockServer.close();
  });
}
