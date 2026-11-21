import { http, type HttpHandler } from 'msw';
import { scriptHandlers } from '../../features/rules/script/scriptMocks';
import { jsonResponse } from './responses';
import { testerHandlers } from './tester';

const VARIABLES = '/api/v1/endpoints/:endpointId/template/variables';
const CHANNELS = '/api/v1/endpoints/:endpointId/notifications';
const SECRETS = '/api/v1/endpoints/:endpointId/secrets';

export const ruleEditorSupportHandlers: HttpHandler[] = [
  http.get(VARIABLES, () => jsonResponse({ delimiters: { open: '{{', close: '}}' }, items: [] })),
  http.get(CHANNELS, () => jsonResponse({ items: [] })),
  http.get(SECRETS, () => jsonResponse({ items: [] })),
  ...scriptHandlers,
  ...testerHandlers,
];
