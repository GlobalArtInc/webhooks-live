import type {
  Body,
  Capture,
  CaptureSummary,
  Endpoint,
  EndpointInput,
  ForwardRule,
  ForwardRuleInput,
  Instance,
  Member,
  User,
} from '../../api/types';
import { instanceFixture } from '../fixtures/instance';

export interface MockStore {
  instance: Instance;
  endpoints: Map<string, Endpoint>;
  captures: Map<string, Capture[]>;
  members: Map<string, Member[]>;
  users: User[];
}

const ENDPOINT_ID_ALPHABET = '0123456789abcdefghjkmnpqrstvwxyz';
const ENDPOINT_ID_LENGTH = 24;
const RETENTION_MS = 7 * 24 * 60 * 60 * 1000;

export const mockStore: MockStore = {
  instance: instanceFixture,
  endpoints: new Map(),
  captures: new Map(),
  members: new Map(),
  users: [],
};

export function resetMockStore(): void {
  mockStore.instance = instanceFixture;
  mockStore.endpoints.clear();
  mockStore.captures.clear();
  mockStore.members.clear();
  mockStore.users = [];
}

export function storeCaptures(endpointId: string, captures: Capture[]): void {
  const sorted = [...captures].sort((left, right) => left.seq - right.seq);
  mockStore.captures.set(endpointId, sorted);
  const endpoint = mockStore.endpoints.get(endpointId);
  if (endpoint !== undefined) {
    mockStore.endpoints.set(endpointId, {
      ...endpoint,
      capture_count: sorted.length,
      bytes_used: sorted.reduce((sum, capture) => sum + capture.body_size, 0),
    });
  }
}

export function seedEndpoint(endpoint: Endpoint, captures: Capture[] = []): void {
  const lastSeq = captures.reduce((max, capture) => Math.max(max, capture.seq), 0);
  mockStore.endpoints.set(endpoint.id, { ...endpoint, last_seq: lastSeq });
  storeCaptures(endpoint.id, captures);
}

export function seedUser(user: User): void {
  mockStore.users = [...mockStore.users, user];
}

export function summaryOf(capture: Capture): CaptureSummary {
  return {
    id: capture.id,
    seq: capture.seq,
    endpoint_id: capture.endpoint_id,
    received_at: capture.received_at,
    method: capture.method,
    path: capture.path,
    query_string: capture.query_string,
    client_ip: capture.client_ip,
    content_type: capture.content_type,
    body_size: capture.body_size,
    status_override: capture.status_override,
    response_status: capture.response_status,
    forward_state: capture.forward_state,
  };
}

export function bodyBytes(body: Body): Uint8Array {
  if (body.encoding === 'base64') {
    return Uint8Array.from(atob(body.data), (character) => character.charCodeAt(0));
  }
  return new TextEncoder().encode(body.data);
}

export function randomEndpointId(): string {
  return Array.from({ length: ENDPOINT_ID_LENGTH }, () =>
    ENDPOINT_ID_ALPHABET.charAt(Math.floor(Math.random() * ENDPOINT_ID_ALPHABET.length)),
  ).join('');
}

export function randomUuid(): string {
  return globalThis.crypto.randomUUID();
}

function toForwardRule(input: ForwardRuleInput): ForwardRule {
  return {
    id: input.id ?? randomUuid(),
    enabled: input.enabled ?? true,
    target_url: input.target_url,
    method: input.method ?? null,
    content_type: input.content_type ?? null,
    header_allowlist: input.header_allowlist ?? [],
    append_path: input.append_path ?? false,
  };
}

export function applyEndpointInput(endpoint: Endpoint, input: EndpointInput): Endpoint {
  const response = input.response ?? {};
  return {
    ...endpoint,
    title: input.title === undefined ? endpoint.title : input.title,
    response: {
      status: response.status ?? endpoint.response.status,
      content_type: response.content_type ?? endpoint.response.content_type,
      headers: response.headers ?? endpoint.response.headers,
      body: response.body === undefined ? endpoint.response.body : { ...response.body, truncated: false },
      delay_ms: response.delay_ms ?? endpoint.response.delay_ms,
    },
    cors: input.cors ?? endpoint.cors,
    overflow: input.overflow ?? endpoint.overflow,
    forward_rules: input.forward_rules === undefined ? endpoint.forward_rules : input.forward_rules.map(toForwardRule),
  };
}

export function newEndpoint(input: EndpointInput): Endpoint {
  const id = randomEndpointId();
  const now = new Date();
  const base: Endpoint = {
    id,
    url: `${mockStore.instance.public_url}/${id}`,
    title: null,
    response: {
      status: 200,
      content_type: 'text/plain',
      headers: [],
      body: { encoding: 'utf8', data: '', truncated: false },
      delay_ms: 0,
    },
    cors: false,
    overflow: 'refuse',
    forward_rules: [],
    max_captures: mockStore.instance.limits.max_captures,
    capture_count: 0,
    bytes_used: 0,
    last_seq: 0,
    created_at: now.toISOString(),
    last_active_at: now.toISOString(),
    expires_at: new Date(now.getTime() + RETENTION_MS).toISOString(),
    owner: null,
    my_role: null,
  };
  return applyEndpointInput(base, input);
}
