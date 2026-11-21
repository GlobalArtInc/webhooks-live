import { isEndpointId } from '../../app/ids';

export const ORGANIZE_STORAGE_KEY = 'webhooks-live:organize:v1';
export const CURRENT_ORGANIZE_VERSION = 1;
export const MAX_LOCAL_FOLDERS = 50;
export const MAX_TAGS_PER_ENDPOINT = 10;
export const MAX_FOLDER_NAME_LENGTH = 64;
export const TAG_PATTERN = /^[a-z0-9][a-z0-9-]{0,31}$/;

const FOLDER_ID_PATTERN = /^[a-z0-9-]{1,64}$/;

type UnknownRecord = Record<string, unknown>;

export interface LocalFolder {
  id: string;
  name: string;
}

export interface LocalPlacement {
  id: string;
  folderId: string | null;
  tags: string[];
}

export interface LocalOrganize {
  folders: LocalFolder[];
  placements: LocalPlacement[];
}

export const EMPTY_ORGANIZE: LocalOrganize = { folders: [], placements: [] };

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function normalizeTag(raw: string): string {
  return raw.trim().toLowerCase();
}

export function isValidTag(tag: string): boolean {
  return TAG_PATTERN.test(tag);
}

export function normalizeFolderName(raw: string): string {
  return raw.trim();
}

function readTags(value: unknown): string[] | null {
  if (!Array.isArray(value)) {
    return null;
  }
  const tags: string[] = [];
  for (const item of value) {
    if (typeof item !== 'string' || !isValidTag(item) || tags.includes(item)) {
      return null;
    }
    tags.push(item);
  }
  return tags.length <= MAX_TAGS_PER_ENDPOINT ? tags : null;
}

function readFolder(value: unknown): LocalFolder | null {
  if (!isRecord(value)) {
    return null;
  }
  const { id, name } = value;
  if (typeof id !== 'string' || !FOLDER_ID_PATTERN.test(id) || typeof name !== 'string') {
    return null;
  }
  const trimmed = normalizeFolderName(name);
  if (trimmed === '' || Array.from(trimmed).length > MAX_FOLDER_NAME_LENGTH) {
    return null;
  }
  return { id, name: trimmed };
}

function readPlacement(value: unknown): LocalPlacement | null {
  if (!isRecord(value)) {
    return null;
  }
  const { id, folderId, tags } = value;
  const readTagList = readTags(tags);
  if (typeof id !== 'string' || !isEndpointId(id) || readTagList === null) {
    return null;
  }
  if (folderId !== null && typeof folderId !== 'string') {
    return null;
  }
  return { id, folderId, tags: readTagList };
}

function isPresent<T>(value: T | null): value is T {
  return value !== null;
}

function uniqueBy<T>(items: readonly T[], keyOf: (item: T) => string): T[] {
  const seen = new Set<string>();
  return items.filter((item) => {
    const key = keyOf(item);
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

export function parseOrganize(raw: string | null): LocalOrganize {
  if (raw === null) {
    return EMPTY_ORGANIZE;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return EMPTY_ORGANIZE;
  }
  if (!isRecord(parsed) || parsed.version !== CURRENT_ORGANIZE_VERSION) {
    return EMPTY_ORGANIZE;
  }
  const folders = Array.isArray(parsed.folders)
    ? uniqueBy(parsed.folders.map(readFolder).filter(isPresent), (folder) => folder.id).slice(0, MAX_LOCAL_FOLDERS)
    : [];
  const folderIds = new Set(folders.map((folder) => folder.id));
  const placements = Array.isArray(parsed.placements)
    ? uniqueBy(parsed.placements.map(readPlacement).filter(isPresent), (placement) => placement.id).map((placement) =>
        placement.folderId !== null && !folderIds.has(placement.folderId)
          ? { ...placement, folderId: null }
          : placement,
      )
    : [];
  return { folders, placements };
}

export function serializeOrganize(state: LocalOrganize): string {
  return JSON.stringify({ version: CURRENT_ORGANIZE_VERSION, folders: state.folders, placements: state.placements });
}

function upsertPlacement(
  state: LocalOrganize,
  endpointId: string,
  change: (placement: LocalPlacement) => LocalPlacement,
): LocalOrganize {
  const existing = state.placements.find((placement) => placement.id === endpointId);
  const base: LocalPlacement = existing ?? { id: endpointId, folderId: null, tags: [] };
  const next = change(base);
  const others = state.placements.filter((placement) => placement.id !== endpointId);
  return { ...state, placements: [...others, next] };
}

export function addLocalFolder(state: LocalOrganize, folder: LocalFolder): LocalOrganize {
  return { ...state, folders: [...state.folders, folder] };
}

export function renameLocalFolder(state: LocalOrganize, folderId: string, name: string): LocalOrganize {
  return {
    ...state,
    folders: state.folders.map((folder) => (folder.id === folderId ? { ...folder, name } : folder)),
  };
}

export function removeLocalFolder(state: LocalOrganize, folderId: string): LocalOrganize {
  return {
    folders: state.folders.filter((folder) => folder.id !== folderId),
    placements: state.placements.map((placement) =>
      placement.folderId === folderId ? { ...placement, folderId: null } : placement,
    ),
  };
}

export function placeLocalEndpoint(state: LocalOrganize, endpointId: string, folderId: string | null): LocalOrganize {
  return upsertPlacement(state, endpointId, (placement) => ({ ...placement, folderId }));
}

export function tagLocalEndpoint(state: LocalOrganize, endpointId: string, tags: readonly string[]): LocalOrganize {
  return upsertPlacement(state, endpointId, (placement) => ({ ...placement, tags: [...tags] }));
}

export function forgetLocalEndpoint(state: LocalOrganize, endpointId: string): LocalOrganize {
  return { ...state, placements: state.placements.filter((placement) => placement.id !== endpointId) };
}
