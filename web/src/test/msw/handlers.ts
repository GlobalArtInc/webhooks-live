import { http, HttpResponse, type HttpHandler } from 'msw';
import type {
  Capture,
  Endpoint,
  EndpointInput,
  ForwardJob,
  ForwardRequest,
  ForwardRule,
  ForwardRuleInput,
  Member,
  MemberInput,
} from '../../api/types';
import { API_HEADERS, emptyResponse, errorResponse, jsonResponse, readJsonBody } from './responses';
import { applyEndpointInput, bodyBytes, mockStore, newEndpoint, randomUuid, storeCaptures, summaryOf } from './store';

interface PageSlice<T> {
  items: T[];
  next_cursor: string | null;
}

interface EndpointPath {
  endpointId: string;
}

interface MemberPath extends EndpointPath {
  userId: string;
}

interface CapturePath extends EndpointPath {
  captureId: string;
}

const DEFAULT_LIMIT = 50;
const MAX_LIMIT = 200;

const INSTANCE = '/api/v1/instance';
const ENDPOINTS = '/api/v1/endpoints';
const ENDPOINT = '/api/v1/endpoints/:endpointId';
const STREAM = '/api/v1/endpoints/:endpointId/stream';
const MEMBERS = '/api/v1/endpoints/:endpointId/members';
const MEMBER = '/api/v1/endpoints/:endpointId/members/:userId';
const CAPTURES = '/api/v1/endpoints/:endpointId/captures';
const LATEST = '/api/v1/endpoints/:endpointId/captures/latest';
const CAPTURE = '/api/v1/endpoints/:endpointId/captures/:captureId';
const CAPTURE_BODY = '/api/v1/endpoints/:endpointId/captures/:captureId/body';
const CAPTURE_FORWARD = '/api/v1/endpoints/:endpointId/captures/:captureId/forward';
const FORWARD_RULES = '/api/v1/endpoints/:endpointId/forward-rules';
const FORWARD_RULE = '/api/v1/endpoints/:endpointId/forward-rules/:ruleId';
const MOCK_RULE_LIMIT = 5;
const REFUSED_TARGET_PREFIX = 'http://10.';

interface RulePath extends EndpointPath {
  ruleId: string;
}

function endpointMissing(): Response {
  return errorResponse(404, 'endpoint_not_found', 'Endpoint not found.');
}

function captureMissing(): Response {
  return errorResponse(404, 'capture_not_found', 'Capture not found.');
}

function invalidJson(): Response {
  return errorResponse(400, 'invalid_json', 'Request body is not valid JSON.');
}

function validationError(field: string, message: string): Response {
  return errorResponse(422, 'validation_failed', `${field} ${message}`, [{ field, message }]);
}

function ssoOnly(): Response | null {
  return mockStore.instance.auth_mode === 'sso'
    ? null
    : errorResponse(403, 'not_available_in_mode', 'This route needs sso mode.');
}

function findCapture(endpointId: string, captureId: string): Capture | undefined {
  return mockStore.captures.get(endpointId)?.find((capture) => capture.id === captureId);
}

function parseLimit(params: URLSearchParams): number | null {
  const raw = params.get('limit');
  if (raw === null) {
    return DEFAULT_LIMIT;
  }
  const value = Number(raw);
  return Number.isInteger(value) && value >= 1 && value <= MAX_LIMIT ? value : null;
}

function sliceAfter<T extends { id: string }>(items: T[], cursor: string | null, limit: number): PageSlice<T> | null {
  const start = cursor === null ? 0 : items.findIndex((item) => item.id === cursor) + 1;
  if (cursor !== null && start === 0) {
    return null;
  }
  const page = items.slice(start, start + limit);
  const hasMore = start + limit < items.length;
  return { items: page, next_cursor: hasMore ? (page.at(-1)?.id ?? null) : null };
}

function matchesStatusClass(override: number | null, statusClass: string): boolean {
  if (statusClass === 'none') {
    return override === null;
  }
  return override !== null && statusClass === `${Math.floor(override / 100)}xx`;
}

function containsText(capture: Capture, needle: string): boolean {
  const parts = [
    capture.path,
    capture.query_string,
    ...capture.headers.flatMap((header) => [header.name, header.value]),
  ];
  if (capture.body.encoding === 'utf8') {
    parts.push(capture.body.data);
  }
  return parts.join('\n').toLowerCase().includes(needle);
}

function matchesFilters(capture: Capture, params: URLSearchParams): boolean {
  const methods = params.get('method')?.split(',') ?? [];
  const kinds = params.get('kind')?.split(',') ?? [];
  const statuses = params.get('status')?.split(',') ?? [];
  const since = params.get('since');
  const until = params.get('until');
  const q = params.get('q');
  const receivedAt = Date.parse(capture.received_at);
  if (methods.length > 0 && !methods.some((method) => method.toUpperCase() === capture.method.toUpperCase())) {
    return false;
  }
  if (kinds.length > 0 && !kinds.includes(capture.kind ?? 'http')) {
    return false;
  }
  if (
    statuses.length > 0 &&
    !statuses.some((statusClass) => matchesStatusClass(capture.status_override, statusClass))
  ) {
    return false;
  }
  if (since !== null && receivedAt < Date.parse(since)) {
    return false;
  }
  if (until !== null && receivedAt >= Date.parse(until)) {
    return false;
  }
  return q === null || containsText(capture, q.toLowerCase());
}

function listCaptureResponse(endpointId: string, params: URLSearchParams): Response {
  const order = params.get('order') ?? 'desc';
  if (order !== 'desc' && order !== 'asc') {
    return validationError('order', 'must be desc or asc');
  }
  const limit = parseLimit(params);
  if (limit === null) {
    return validationError('limit', 'must be between 1 and 200');
  }
  const q = params.get('q');
  if (q !== null && (q.length < 1 || q.length > MAX_LIMIT)) {
    return validationError('q', 'must be 1 to 200 characters');
  }
  const matching = (mockStore.captures.get(endpointId) ?? []).filter((capture) => matchesFilters(capture, params));
  const ordered = order === 'desc' ? [...matching].reverse() : matching;
  const slice = sliceAfter(ordered, params.get('cursor'), limit);
  if (slice === null) {
    return errorResponse(400, 'invalid_cursor', 'Cursor is not valid for this query.');
  }
  return jsonResponse({ items: slice.items.map(summaryOf), next_cursor: slice.next_cursor, total: matching.length });
}

function isVisibleTo(endpoint: Endpoint, role: string | null, userId: string): boolean {
  const owned = endpoint.owner?.id === userId;
  const shared = (mockStore.members.get(endpoint.id) ?? []).some((member) => member.user.id === userId);
  if (role === 'owner') {
    return owned;
  }
  if (role === 'shared') {
    return shared;
  }
  return owned || shared;
}

function listEndpointResponse(params: URLSearchParams): Response {
  const denied = ssoOnly();
  if (denied !== null) {
    return denied;
  }
  const user = mockStore.instance.user;
  if (user === null) {
    return errorResponse(401, 'unauthenticated', 'Sign in to continue.');
  }
  const limit = parseLimit(params);
  if (limit === null) {
    return validationError('limit', 'must be between 1 and 200');
  }
  const role = params.get('role');
  const visible = [...mockStore.endpoints.values()]
    .filter((endpoint) => isVisibleTo(endpoint, role, user.id))
    .sort((left, right) => right.last_active_at.localeCompare(left.last_active_at));
  const slice = sliceAfter(visible, params.get('cursor'), limit);
  if (slice === null) {
    return errorResponse(400, 'invalid_cursor', 'Cursor is not valid for this query.');
  }
  return jsonResponse({ items: slice.items, next_cursor: slice.next_cursor });
}

function withOwner(endpoint: Endpoint): Endpoint {
  const user = mockStore.instance.user;
  return user === null ? endpoint : { ...endpoint, owner: user, my_role: 'owner' };
}

function replaceCapture(endpointId: string, capture: Capture): void {
  const captures = mockStore.captures.get(endpointId) ?? [];
  storeCaptures(
    endpointId,
    captures.map((existing) => (existing.id === capture.id ? capture : existing)),
  );
}

function newForwardJob(capture: Capture, input: ForwardRequest, targetUrl: string): ForwardJob {
  return {
    id: randomUuid(),
    kind: 'manual',
    rule_id: null,
    target_url: targetUrl,
    method: input.method ?? capture.method,
    state: 'pending',
    attempts: [],
    created_at: new Date().toISOString(),
    finished_at: null,
  };
}

function ruleTargetRefusal(targetUrl: string): Response | null {
  return targetUrl.startsWith(REFUSED_TARGET_PREFIX)
    ? errorResponse(422, 'validation_failed', 'target_url target resolves only to refused addresses', [
        { field: 'target_url', code: 'target_refused', message: '10.0.0.5 is a private address' },
      ])
    : null;
}

function ruleFrom(input: ForwardRuleInput, existing: ForwardRule | undefined): ForwardRule {
  return {
    id: existing?.id ?? input.id ?? randomUuid(),
    enabled: input.enabled ?? existing?.enabled ?? true,
    target_url: input.target_url ?? existing?.target_url ?? '',
    method: input.method === undefined ? (existing?.method ?? null) : input.method,
    content_type: input.content_type === undefined ? (existing?.content_type ?? null) : input.content_type,
    header_allowlist: input.header_allowlist ?? existing?.header_allowlist ?? [],
    append_path: input.append_path ?? existing?.append_path ?? false,
  };
}

function withRules(endpointId: string, rules: ForwardRule[]): Endpoint | undefined {
  const endpoint = mockStore.endpoints.get(endpointId);
  if (endpoint === undefined) {
    return undefined;
  }
  const updated = { ...endpoint, forward_rules: rules };
  mockStore.endpoints.set(endpointId, updated);
  return updated;
}

function streamReady(endpoint: Endpoint): Response {
  const data = JSON.stringify({ endpoint_id: endpoint.id, last_seq: endpoint.last_seq });
  return new HttpResponse(`retry: 2000\n\nevent: ready\ndata: ${data}\n\n`, {
    headers: { ...API_HEADERS, 'Content-Type': 'text/event-stream', 'X-Accel-Buffering': 'no' },
  });
}

function rawBody(capture: Capture, params: URLSearchParams): Response {
  const asText = params.get('view') === 'text';
  const download = params.get('download') === '1';
  return new HttpResponse(bodyBytes(capture.body), {
    headers: {
      ...API_HEADERS,
      'Content-Type': asText ? 'text/plain; charset=utf-8' : 'application/octet-stream',
      'Content-Disposition': download ? `attachment; filename="${capture.id}.bin"` : 'inline',
      'Content-Security-Policy': "sandbox; default-src 'none'",
      'X-Content-Type-Options': 'nosniff',
      'Cross-Origin-Resource-Policy': 'same-origin',
    },
  });
}

export const handlers: HttpHandler[] = [
  http.get(INSTANCE, () => jsonResponse(mockStore.instance)),

  http.post(ENDPOINTS, async ({ request }) => {
    const input = await readJsonBody<EndpointInput>(request, {});
    if (input === null) {
      return invalidJson();
    }
    const created = withOwner(newEndpoint(input));
    mockStore.endpoints.set(created.id, created);
    return jsonResponse(created, 201, { Location: `/api/v1/endpoints/${created.id}` });
  }),

  http.get(ENDPOINTS, ({ request }) => listEndpointResponse(new URL(request.url).searchParams)),

  http.get<EndpointPath>(ENDPOINT, ({ params }) => {
    const endpoint = mockStore.endpoints.get(params.endpointId);
    return endpoint === undefined ? endpointMissing() : jsonResponse(endpoint);
  }),

  http.patch<EndpointPath>(ENDPOINT, async ({ params, request }) => {
    const endpoint = mockStore.endpoints.get(params.endpointId);
    if (endpoint === undefined) {
      return endpointMissing();
    }
    const input = await readJsonBody<EndpointInput>(request);
    if (input === null) {
      return invalidJson();
    }
    const updated = applyEndpointInput(endpoint, input);
    mockStore.endpoints.set(updated.id, updated);
    return jsonResponse(updated);
  }),

  http.delete<EndpointPath>(ENDPOINT, ({ params }) => {
    if (!mockStore.endpoints.has(params.endpointId)) {
      return endpointMissing();
    }
    mockStore.endpoints.delete(params.endpointId);
    mockStore.captures.delete(params.endpointId);
    mockStore.members.delete(params.endpointId);
    return emptyResponse();
  }),

  http.get<EndpointPath>(MEMBERS, ({ params }) => {
    const denied = ssoOnly();
    if (denied !== null) {
      return denied;
    }
    if (!mockStore.endpoints.has(params.endpointId)) {
      return endpointMissing();
    }
    return jsonResponse({ items: mockStore.members.get(params.endpointId) ?? [] });
  }),

  http.put<EndpointPath>(MEMBERS, async ({ params, request }) => {
    const denied = ssoOnly();
    if (denied !== null) {
      return denied;
    }
    const endpoint = mockStore.endpoints.get(params.endpointId);
    if (endpoint === undefined) {
      return endpointMissing();
    }
    const input = await readJsonBody<MemberInput>(request);
    if (input === null) {
      return invalidJson();
    }
    const user = mockStore.users.find((candidate) => candidate.email === input.email);
    if (user === undefined) {
      return errorResponse(404, 'user_not_found', 'No user with this email has signed in.');
    }
    if (endpoint.owner?.id === user.id) {
      return validationError('email', 'is the owner of this endpoint');
    }
    const members = mockStore.members.get(endpoint.id) ?? [];
    const existing = members.find((member) => member.user.id === user.id);
    const member: Member = {
      user,
      role: input.role,
      added_at: existing?.added_at ?? new Date().toISOString(),
    };
    mockStore.members.set(endpoint.id, [...members.filter((other) => other.user.id !== user.id), member]);
    return jsonResponse(member);
  }),

  http.delete<MemberPath>(MEMBER, ({ params }) => {
    const denied = ssoOnly();
    if (denied !== null) {
      return denied;
    }
    if (!mockStore.endpoints.has(params.endpointId)) {
      return endpointMissing();
    }
    const members = mockStore.members.get(params.endpointId) ?? [];
    if (!members.some((member) => member.user.id === params.userId)) {
      return errorResponse(404, 'not_found', 'Member not found.');
    }
    mockStore.members.set(
      params.endpointId,
      members.filter((member) => member.user.id !== params.userId),
    );
    return emptyResponse();
  }),

  http.get<EndpointPath>(CAPTURES, ({ params, request }) => {
    if (!mockStore.endpoints.has(params.endpointId)) {
      return endpointMissing();
    }
    return listCaptureResponse(params.endpointId, new URL(request.url).searchParams);
  }),

  http.delete<EndpointPath>(CAPTURES, ({ params }) => {
    if (!mockStore.endpoints.has(params.endpointId)) {
      return endpointMissing();
    }
    storeCaptures(params.endpointId, []);
    return emptyResponse();
  }),

  http.get<EndpointPath>(LATEST, ({ params }) => {
    if (!mockStore.endpoints.has(params.endpointId)) {
      return endpointMissing();
    }
    const latest = mockStore.captures.get(params.endpointId)?.at(-1);
    return latest === undefined ? captureMissing() : jsonResponse(latest);
  }),

  http.get<CapturePath>(CAPTURE, ({ params }) => {
    const capture = findCapture(params.endpointId, params.captureId);
    return capture === undefined ? captureMissing() : jsonResponse(capture);
  }),

  http.delete<CapturePath>(CAPTURE, ({ params }) => {
    const capture = findCapture(params.endpointId, params.captureId);
    if (capture === undefined) {
      return captureMissing();
    }
    const remaining = (mockStore.captures.get(params.endpointId) ?? []).filter((other) => other.id !== capture.id);
    storeCaptures(params.endpointId, remaining);
    return emptyResponse();
  }),

  http.get<CapturePath>(CAPTURE_BODY, ({ params, request }) => {
    const capture = findCapture(params.endpointId, params.captureId);
    if (capture === undefined) {
      return captureMissing();
    }
    return rawBody(capture, new URL(request.url).searchParams);
  }),

  http.post<CapturePath>(CAPTURE_FORWARD, async ({ params, request }) => {
    const capture = findCapture(params.endpointId, params.captureId);
    if (capture === undefined) {
      return captureMissing();
    }
    const input = await readJsonBody<ForwardRequest>(request);
    if (input === null) {
      return invalidJson();
    }
    if (typeof input.target_url !== 'string') {
      return validationError('target_url', 'is required');
    }
    const rule =
      input.rule_id === undefined
        ? undefined
        : mockStore.endpoints.get(params.endpointId)?.forward_rules.find((item) => item.id === input.rule_id);
    if (input.rule_id !== undefined && rule === undefined) {
      return errorResponse(404, 'forward_rule_not_found', 'Forward rule not found.');
    }
    const targetUrl = rule?.target_url ?? input.target_url;
    if (typeof targetUrl !== 'string') {
      return validationError('target_url', 'is required');
    }
    const job: ForwardJob = {
      ...newForwardJob(capture, input, targetUrl),
      kind: rule === undefined ? 'manual' : 'rule',
      rule_id: rule?.id ?? null,
    };
    replaceCapture(params.endpointId, {
      ...capture,
      forward_state: 'pending',
      forwards: [job, ...capture.forwards],
    });
    return jsonResponse(job, 202);
  }),

  http.post<EndpointPath>(FORWARD_RULES, async ({ params, request }) => {
    const endpoint = mockStore.endpoints.get(params.endpointId);
    if (endpoint === undefined) {
      return endpointMissing();
    }
    const input = await readJsonBody<ForwardRuleInput>(request);
    if (input === null) {
      return invalidJson();
    }
    if (endpoint.forward_rules.length >= MOCK_RULE_LIMIT) {
      return errorResponse(422, 'forward_rule_limit', 'Endpoint already has the maximum number of rules.');
    }
    const refusal = ruleTargetRefusal(input.target_url ?? '');
    if (refusal !== null) {
      return refusal;
    }
    const rule = ruleFrom(input, undefined);
    withRules(params.endpointId, [...endpoint.forward_rules, rule]);
    return jsonResponse(rule, 201);
  }),

  http.patch<RulePath>(FORWARD_RULE, async ({ params, request }) => {
    const endpoint = mockStore.endpoints.get(params.endpointId);
    if (endpoint === undefined) {
      return endpointMissing();
    }
    const existing = endpoint.forward_rules.find((rule) => rule.id === params.ruleId);
    if (existing === undefined) {
      return errorResponse(404, 'forward_rule_not_found', 'Forward rule not found.');
    }
    const input = await readJsonBody<ForwardRuleInput>(request);
    if (input === null) {
      return invalidJson();
    }
    if (input.target_url !== undefined) {
      const refusal = ruleTargetRefusal(input.target_url);
      if (refusal !== null) {
        return refusal;
      }
    }
    const rule = ruleFrom(input, existing);
    withRules(
      params.endpointId,
      endpoint.forward_rules.map((item) => (item.id === rule.id ? rule : item)),
    );
    return jsonResponse(rule);
  }),

  http.delete<RulePath>(FORWARD_RULE, ({ params }) => {
    const endpoint = mockStore.endpoints.get(params.endpointId);
    if (endpoint === undefined) {
      return endpointMissing();
    }
    if (!endpoint.forward_rules.some((rule) => rule.id === params.ruleId)) {
      return errorResponse(404, 'forward_rule_not_found', 'Forward rule not found.');
    }
    withRules(
      params.endpointId,
      endpoint.forward_rules.filter((rule) => rule.id !== params.ruleId),
    );
    return emptyResponse();
  }),

  http.get<EndpointPath>(STREAM, ({ params }) => {
    const endpoint = mockStore.endpoints.get(params.endpointId);
    return endpoint === undefined ? endpointMissing() : streamReady(endpoint);
  }),
];
