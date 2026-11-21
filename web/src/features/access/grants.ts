import { setGrantResolver } from '../../api/client';

interface StoredGrant {
  grant: string;
  expiresAt: number;
}

const STORAGE_PREFIX = 'wl.grant.';
const memory = new Map<string, StoredGrant>();

function sessionStore(): Storage | null {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

function persist(endpointId: string, entry: StoredGrant): void {
  try {
    sessionStore()?.setItem(storageKey(endpointId), JSON.stringify(entry));
  } catch {
    memory.set(endpointId, entry);
  }
}

function parseStored(text: string | null): StoredGrant | null {
  if (text === null) {
    return null;
  }
  try {
    const value: unknown = JSON.parse(text);
    if (typeof value === 'object' && value !== null && 'grant' in value && 'expiresAt' in value) {
      const { grant, expiresAt } = value;
      return typeof grant === 'string' && typeof expiresAt === 'number' ? { grant, expiresAt } : null;
    }
    return null;
  } catch {
    return null;
  }
}

function storageKey(endpointId: string): string {
  return `${STORAGE_PREFIX}${endpointId}`;
}

export function storeGrant(endpointId: string, grant: string, expiresAt: string): boolean {
  const expires = Date.parse(expiresAt);
  if (grant === '' || Number.isNaN(expires)) {
    return false;
  }
  const entry: StoredGrant = { grant, expiresAt: expires };
  memory.set(endpointId, entry);
  persist(endpointId, entry);
  return true;
}

export function clearGrant(endpointId: string): void {
  memory.delete(endpointId);
  sessionStore()?.removeItem(storageKey(endpointId));
}

export function readGrant(endpointId: string, now: number = Date.now()): string | null {
  const entry = memory.get(endpointId) ?? parseStored(sessionStore()?.getItem(storageKey(endpointId)) ?? null);
  if (entry === null) {
    return null;
  }
  if (entry.expiresAt <= now) {
    clearGrant(endpointId);
    return null;
  }
  memory.set(endpointId, entry);
  return entry.grant;
}

export function installGrantResolver(): void {
  setGrantResolver((endpointId) => readGrant(endpointId));
}
