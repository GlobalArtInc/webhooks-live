import { isEndpointId } from '../../app/ids';

export const ENDPOINTS_STORAGE_KEY = 'webhooks-live:endpoints:v1';
export const CURRENT_ENDPOINTS_VERSION = 1;
export const MAX_MY_ENDPOINTS = 50;
export const MAX_LABEL_LENGTH = 100;

type UnknownRecord = Record<string, unknown>;

export interface MyEndpoint {
  id: string;
  label: string | null;
  added: number;
  lastOpened: number;
  lastSeenCaptureId: string | null;
}

export interface StoredEndpoints {
  version: number;
  endpoints: MyEndpoint[];
}

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isTimestamp(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function isPresent(entry: MyEndpoint | null): entry is MyEndpoint {
  return entry !== null;
}

export function normalizeLabel(label: string): string | null {
  const trimmed = label.trim();
  if (trimmed === '') {
    return null;
  }
  return Array.from(trimmed).slice(0, MAX_LABEL_LENGTH).join('');
}

function readEntry(value: unknown): MyEndpoint | null {
  if (!isRecord(value)) {
    return null;
  }
  const { id, label, added, lastOpened, lastSeenCaptureId } = value;
  if (typeof id !== 'string' || !isEndpointId(id) || !isTimestamp(added) || !isTimestamp(lastOpened)) {
    return null;
  }
  return {
    id,
    label: typeof label === 'string' ? normalizeLabel(label) : null,
    added,
    lastOpened,
    lastSeenCaptureId: typeof lastSeenCaptureId === 'string' ? lastSeenCaptureId : null,
  };
}

function uniqueById(endpoints: readonly MyEndpoint[]): MyEndpoint[] {
  const seen = new Set<string>();
  return endpoints.filter((entry) => {
    if (seen.has(entry.id)) {
      return false;
    }
    seen.add(entry.id);
    return true;
  });
}

function readEntries(value: unknown): MyEndpoint[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const entries = value.map(readEntry).filter(isPresent);
  return capEndpoints(uniqueById(entries));
}

export function migrateEndpoints(value: unknown): MyEndpoint[] {
  if (!isRecord(value)) {
    return [];
  }
  switch (value.version) {
    case CURRENT_ENDPOINTS_VERSION:
      return readEntries(value.endpoints);
    default:
      return [];
  }
}

export function parseEndpoints(raw: string | null): MyEndpoint[] {
  if (raw === null) {
    return [];
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    return migrateEndpoints(parsed);
  } catch {
    return [];
  }
}

export function serializeEndpoints(endpoints: readonly MyEndpoint[]): string {
  const stored: StoredEndpoints = { version: CURRENT_ENDPOINTS_VERSION, endpoints: [...endpoints] };
  return JSON.stringify(stored);
}

export function capEndpoints(endpoints: readonly MyEndpoint[]): MyEndpoint[] {
  const overflow = endpoints.length - MAX_MY_ENDPOINTS;
  if (overflow <= 0) {
    return [...endpoints];
  }
  const evicted = new Set(
    endpoints
      .map((entry, index) => ({ index, lastOpened: entry.lastOpened }))
      .sort((left, right) => left.lastOpened - right.lastOpened || right.index - left.index)
      .slice(0, overflow)
      .map((candidate) => candidate.index),
  );
  return endpoints.filter((_entry, index) => !evicted.has(index));
}

export function addEndpoint(endpoints: readonly MyEndpoint[], id: string, now: number): MyEndpoint[] {
  if (endpoints.some((entry) => entry.id === id)) {
    return touchEndpoint(endpoints, id, now);
  }
  const entry: MyEndpoint = { id, label: null, added: now, lastOpened: now, lastSeenCaptureId: null };
  return capEndpoints([entry, ...endpoints]);
}

export function removeEndpoint(endpoints: readonly MyEndpoint[], id: string): MyEndpoint[] {
  return endpoints.filter((entry) => entry.id !== id);
}

export function renameEndpoint(endpoints: readonly MyEndpoint[], id: string, label: string): MyEndpoint[] {
  const normalized = normalizeLabel(label);
  return endpoints.map((entry) => (entry.id === id ? { ...entry, label: normalized } : entry));
}

export function touchEndpoint(endpoints: readonly MyEndpoint[], id: string, now: number): MyEndpoint[] {
  return endpoints.map((entry) => (entry.id === id ? { ...entry, lastOpened: now } : entry));
}

export function recordLastSeenCapture(endpoints: readonly MyEndpoint[], id: string, captureId: string): MyEndpoint[] {
  return endpoints.map((entry) => (entry.id === id ? { ...entry, lastSeenCaptureId: captureId } : entry));
}

export function byRecentlyOpened(endpoints: readonly MyEndpoint[]): MyEndpoint[] {
  return [...endpoints].sort((left, right) => right.lastOpened - left.lastOpened);
}
