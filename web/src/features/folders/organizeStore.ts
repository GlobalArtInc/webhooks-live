import { useSyncExternalStore } from 'react';
import {
  addLocalFolder,
  EMPTY_ORGANIZE,
  forgetLocalEndpoint,
  ORGANIZE_STORAGE_KEY,
  parseOrganize,
  placeLocalEndpoint,
  removeLocalFolder,
  renameLocalFolder,
  serializeOrganize,
  tagLocalEndpoint,
  type LocalOrganize,
} from './localOrganize';

type Listener = () => void;

interface Snapshot {
  raw: string | null;
  value: LocalOrganize;
}

const listeners = new Set<Listener>();
let snapshot: Snapshot | null = null;
let unsavedRaw: string | null = null;

function readRaw(): string | null {
  try {
    return window.localStorage.getItem(ORGANIZE_STORAGE_KEY) ?? unsavedRaw;
  } catch {
    return unsavedRaw;
  }
}

function writeRaw(raw: string): void {
  try {
    window.localStorage.setItem(ORGANIZE_STORAGE_KEY, raw);
    unsavedRaw = null;
  } catch {
    unsavedRaw = raw;
  }
}

function currentOrganize(): LocalOrganize {
  const raw = readRaw();
  if (snapshot === null || snapshot.raw !== raw) {
    snapshot = { raw, value: parseOrganize(raw) };
  }
  return snapshot.value;
}

function notify(): void {
  for (const listener of listeners) {
    listener();
  }
}

function handleStorage(event: StorageEvent): void {
  if (event.key !== null && event.key !== ORGANIZE_STORAGE_KEY) {
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

function update(change: (state: LocalOrganize) => LocalOrganize): void {
  writeRaw(serializeOrganize(change(currentOrganize())));
  notify();
}

function newFolderId(): string {
  return `f${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

export function useLocalOrganize(): LocalOrganize {
  return useSyncExternalStore(subscribe, currentOrganize, () => EMPTY_ORGANIZE);
}

export function createLocalFolder(name: string): string {
  const id = newFolderId();
  update((state) => addLocalFolder(state, { id, name }));
  return id;
}

export function renameLocalFolderName(folderId: string, name: string): void {
  update((state) => renameLocalFolder(state, folderId, name));
}

export function deleteLocalFolder(folderId: string): void {
  update((state) => removeLocalFolder(state, folderId));
}

export function placeLocalEndpointIn(endpointId: string, folderId: string | null): void {
  update((state) => placeLocalEndpoint(state, endpointId, folderId));
}

export function setLocalEndpointTags(endpointId: string, tags: readonly string[]): void {
  update((state) => tagLocalEndpoint(state, endpointId, tags));
}

export function forgetLocalEndpointOrganize(endpointId: string): void {
  update((state) => forgetLocalEndpoint(state, endpointId));
}
