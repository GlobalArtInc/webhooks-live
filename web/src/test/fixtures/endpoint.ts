import type { Endpoint, EndpointIdentityKey, EndpointView } from '../../api/types';
import { PUBLIC_URL } from './instance';

export const ENDPOINT_ID = '7k2m9q4xw8c1d5f3h6j0n2p4';

export const endpointFixture: Endpoint = {
  id: ENDPOINT_ID,
  url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
  title: 'stripe test',
  tags: [],
  response: {
    status: 202,
    content_type: 'application/json',
    headers: [],
    body: { encoding: 'utf8', data: '{"ok":true}', truncated: false },
    delay_ms: 500,
  },
  cors: true,
  overflow: 'refuse',
  forward_rules: [],
  max_captures: 500,
  capture_count: 0,
  bytes_used: 0,
  last_seq: 0,
  created_at: '2026-11-21T10:15:30.123Z',
  last_active_at: '2026-11-21T10:15:30.123Z',
  expires_at: '2026-11-28T10:15:30.123Z',
  owner: null,
  my_role: null,
};

const IDENTITY_KEYS: readonly EndpointIdentityKey[] = ['tags', 'owner', 'my_role', 'view_grant'];

export function endpointViewOf(endpoint: Endpoint): EndpointView {
  const view: Endpoint = { ...endpoint };
  for (const key of IDENTITY_KEYS) {
    Reflect.deleteProperty(view, key);
  }
  return view;
}
