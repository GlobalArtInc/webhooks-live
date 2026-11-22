import { useSyncExternalStore } from 'react';
import {
  addEndpoint,
  ENDPOINTS_STORAGE_KEY,
  parseEndpoints,
  recordLastSeenCapture,
  removeEndpoint,
  renameEndpoint,
  serializeEndpoints,
  touchEndpoint,
  withEndpointTitle,
  type MyEndpoint,
} from './storage';

type Listener = () => void;

interface Snapshot {
  raw: string | null;
  endpoints: readonly MyEndpoint[];
}

const NO_ENDPOINTS: readonly MyEndpoint[] = [];
const listeners = new Set<Listener>();
let snapshot: Snapshot | null = null;
let unsavedRaw: string | null = null;

function readRaw(): string | null {
  try {
    return window.localStorage.getItem(ENDPOINTS_STORAGE_KEY) ?? unsavedRaw;
  } catch {
    return unsavedRaw;
  }
}

function writeRaw(raw: string): void {
  try {
    window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, raw);
    unsavedRaw = null;
  } catch {
    unsavedRaw = raw;
  }
}

function currentEndpoints(): readonly MyEndpoint[] {
  const raw = readRaw();
  if (snapshot === null || snapshot.raw !== raw) {
    snapshot = { raw, endpoints: parseEndpoints(raw) };
  }
  return snapshot.endpoints;
}

function notify(): void {
  for (const listener of listeners) {
    listener();
  }
}

function handleStorage(event: StorageEvent): void {
  if (event.key !== null && event.key !== ENDPOINTS_STORAGE_KEY) {
    return;
  }
  unsavedRaw = null;
  notify();
}

function subscribe(listener: Listener): () => void {
  if (listeners.size === 0) {
    window.addEventListener('storage', handleStorage);
  }
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      window.removeEventListener('storage', handleStorage);
    }
  };
}

function update(change: (endpoints: readonly MyEndpoint[]) => MyEndpoint[]): void {
  writeRaw(serializeEndpoints(change(currentEndpoints())));
  notify();
}

export function useMyEndpoints(): readonly MyEndpoint[] {
  return useSyncExternalStore(subscribe, currentEndpoints, () => NO_ENDPOINTS);
}

export function rememberEndpoint(id: string, title: string | null = null): void {
  update((endpoints) => addEndpoint(endpoints, id, Date.now(), title));
}

export function recordEndpointTitle(id: string, title: string | null): void {
  const current = currentEndpoints();
  const next = withEndpointTitle(current, id, title);
  if (next.every((entry, index) => entry === current[index])) {
    return;
  }
  writeRaw(serializeEndpoints(next));
  notify();
}

export function forgetEndpoint(id: string): void {
  update((endpoints) => removeEndpoint(endpoints, id));
}

export function renameMyEndpoint(id: string, label: string): void {
  update((endpoints) => renameEndpoint(endpoints, id, label));
}

export function markEndpointOpened(id: string): void {
  update((endpoints) => touchEndpoint(endpoints, id, Date.now()));
}

export function recordEndpointLastSeen(id: string, captureId: string): void {
  update((endpoints) => recordLastSeenCapture(endpoints, id, captureId));
}
