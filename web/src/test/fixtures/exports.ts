export const EXPORT_TITLE = 'Example export';
export const EXPORTED_AT = '2026-11-20T10:00:00Z';

const CAPTURE_ONE = {
  id: 'capture-one',
  kind: 'http',
  seq: 1,
  method: 'POST',
  path: '/hooks/example',
  received_at: '2026-11-20T09:59:00Z',
  headers: [{ name: 'content-type', value: 'application/json' }],
  body: 'eyJleGFtcGxlIjogdHJ1ZX0=',
};

const CAPTURE_TWO = {
  id: 'capture-two',
  kind: 'http',
  seq: 2,
  method: 'GET',
  path: '/hooks/example?ping=1',
  received_at: '2026-11-20T09:59:30Z',
  headers: [],
  body: '',
};

const HEADER = {
  format: 'webhooks-live-export',
  version: 1,
  exported_at: EXPORTED_AT,
  endpoint: { id: 'example-endpoint', title: EXPORT_TITLE, tags: [] },
};

export function jsonExportText(): string {
  return JSON.stringify({ ...HEADER, captures: [CAPTURE_ONE, CAPTURE_TWO] }, null, 2);
}

export function ndjsonExportText(): string {
  return [HEADER, CAPTURE_ONE, CAPTURE_TWO].map((line) => JSON.stringify(line)).join('\n');
}
