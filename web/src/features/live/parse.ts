import type {
  CaptureDeletedPayload,
  CaptureSummary,
  EndpointView,
  ForwardJob,
  ForwardResultPayload,
  ForwardState,
  ResyncReason,
} from '../../api/types';
import { readRuleResult } from '../rules/runlogs/parse';
import { LIVE_EVENT_NAMES, type LiveEvent, type LiveEventName, type LiveReadyPayload } from './types';

type UnknownRecord = Record<string, unknown>;

const FORWARD_STATES: readonly unknown[] = ['pending', 'succeeded', 'failed'];
const JOB_STATES: readonly unknown[] = ['pending', 'running', 'succeeded', 'failed'];
const OVERFLOWS: readonly unknown[] = ['refuse', 'ring'];
const RESYNC_REASONS: readonly unknown[] = ['lagged', 'gap', 'listener_reconnect'];

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === 'string';
}

function isNullableNumber(value: unknown): value is number | null {
  return value === null || typeof value === 'number';
}

function isForwardState(value: unknown): value is ForwardState {
  return FORWARD_STATES.includes(value);
}

function isResyncReason(value: unknown): value is ResyncReason {
  return RESYNC_REASONS.includes(value);
}

function isEndpointView(value: unknown): value is EndpointView {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.url === 'string' &&
    isCount(value.capture_count) &&
    isCount(value.max_captures) &&
    isCount(value.last_seq) &&
    isCount(value.bytes_used) &&
    OVERFLOWS.includes(value.overflow) &&
    isRecord(value.response)
  );
}

function isForwardJob(value: unknown): value is ForwardJob {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    (value.kind === 'rule' || value.kind === 'manual') &&
    JOB_STATES.includes(value.state) &&
    Array.isArray(value.attempts)
  );
}

export function isLiveEventName(value: string): value is LiveEventName {
  return LIVE_EVENT_NAMES.some((name) => name === value);
}

export function readCaptureSummary(value: unknown): CaptureSummary | null {
  if (!isRecord(value)) {
    return null;
  }
  const { id, seq, endpoint_id, received_at, method, path, query_string, client_ip, content_type } = value;
  const { body_size, status_override, response_status, forward_state } = value;
  const forwardState = forward_state === undefined ? null : forward_state;
  if (
    typeof id !== 'string' ||
    !isCount(seq) ||
    typeof endpoint_id !== 'string' ||
    typeof received_at !== 'string' ||
    typeof method !== 'string' ||
    typeof path !== 'string' ||
    typeof query_string !== 'string' ||
    typeof client_ip !== 'string' ||
    !isNullableString(content_type) ||
    !isCount(body_size) ||
    !isNullableNumber(status_override) ||
    typeof response_status !== 'number' ||
    (forwardState !== null && !isForwardState(forwardState))
  ) {
    return null;
  }
  return {
    id,
    seq,
    endpoint_id,
    received_at,
    method,
    path,
    query_string,
    client_ip,
    content_type,
    body_size,
    status_override,
    response_status,
    forward_state: forwardState,
  };
}

function readReady(value: unknown): LiveReadyPayload | null {
  if (!isRecord(value)) {
    return null;
  }
  const { endpoint_id, last_seq, capture_count } = value;
  if (typeof endpoint_id !== 'string' || !isCount(last_seq) || !isCount(capture_count)) {
    return null;
  }
  return { endpoint_id, last_seq, capture_count };
}

function readDeleted(value: unknown): CaptureDeletedPayload | null {
  if (!isRecord(value) || typeof value.capture_id !== 'string' || !isCount(value.seq)) {
    return null;
  }
  return { capture_id: value.capture_id, seq: value.seq };
}

function readForwardResult(value: unknown): ForwardResultPayload | null {
  if (!isRecord(value) || typeof value.capture_id !== 'string' || !isForwardJob(value.job)) {
    return null;
  }
  return { capture_id: value.capture_id, job: value.job };
}

export function readLiveEvent(name: LiveEventName, value: unknown): LiveEvent | null {
  switch (name) {
    case 'ready': {
      const data = readReady(value);
      return data === null ? null : { name, data };
    }
    case 'capture': {
      const data = readCaptureSummary(value);
      return data === null ? null : { name, data };
    }
    case 'capture_deleted': {
      const data = readDeleted(value);
      return data === null ? null : { name, data };
    }
    case 'endpoint_updated':
      return isEndpointView(value) ? { name, data: value } : null;
    case 'forward_result': {
      const data = readForwardResult(value);
      return data === null ? null : { name, data };
    }
    case 'rule_result': {
      const data = readRuleResult(value);
      return data === null ? null : { name, data };
    }
    case 'resync':
      return isRecord(value) && isResyncReason(value.reason) ? { name, data: { reason: value.reason } } : null;
    case 'captures_cleared':
      return { name, data: {} };
    case 'endpoint_deleted':
      return { name, data: {} };
    case 'bye':
      return { name, data: {} };
  }
}

export function parseLiveEvent(name: LiveEventName, text: string): LiveEvent | null {
  if (name === 'bye' || name === 'endpoint_deleted' || name === 'captures_cleared') {
    return { name, data: {} };
  }
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  return readLiveEvent(name, value);
}
