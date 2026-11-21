import type { Endpoint, EndpointRole, Folder } from '../../api/types';
import { shortEndpointId } from '../endpoints/format';
import type { MyEndpoint } from '../endpoints/storage';
import { MAX_FOLDER_NAME_LENGTH, normalizeFolderName, type LocalFolder, type LocalOrganize } from './localOrganize';

export type EndpointSource = 'local' | 'account' | 'shared';
export type SourceFilter = 'local' | 'account' | 'shared';
export type FolderSource = 'local' | 'account';

export const UNFILED_FILTER = 'unfiled';

export interface FolderView {
  key: string;
  source: FolderSource;
  id: string;
  name: string;
}

export interface OrganizedEndpoint {
  id: string;
  source: EndpointSource;
  role: EndpointRole | null;
  label: string | null;
  folderKey: string | null;
  tags: string[];
  captureCount: number | null;
  lastActivity: number;
}

export interface OrganizeInput {
  local: readonly MyEndpoint[];
  organize: LocalOrganize;
  account: readonly Endpoint[];
  knownAccountIds: ReadonlySet<string>;
  accountFolders: readonly Folder[];
}

export interface RailGroup {
  folder: FolderView | null;
  rows: OrganizedEndpoint[];
}

export interface OrganizeFilter {
  folder?: string;
  label?: string;
  source?: SourceFilter;
}

export type NameProblem = 'empty' | 'tooLong' | 'duplicate';

export function localFolderKey(id: string): string {
  return `local:${id}`;
}

export function accountFolderKey(id: string): string {
  return `account:${id}`;
}

export function folderIdFromKey(key: string): string {
  return key.slice(key.indexOf(':') + 1);
}

export function rowName(row: OrganizedEndpoint): string {
  return row.label === null || row.label.trim() === '' ? shortEndpointId(row.id) : row.label;
}

export function folderNameProblem(raw: string, taken: readonly string[]): NameProblem | null {
  const name = normalizeFolderName(raw);
  if (name === '') {
    return 'empty';
  }
  if (Array.from(name).length > MAX_FOLDER_NAME_LENGTH) {
    return 'tooLong';
  }
  const lowered = name.toLowerCase();
  return taken.some((existing) => existing.toLowerCase() === lowered) ? 'duplicate' : null;
}

export function accountFolderViews(folders: readonly Folder[]): FolderView[] {
  return [...folders]
    .sort((left, right) => left.position - right.position || left.name.localeCompare(right.name))
    .map((folder) => ({ key: accountFolderKey(folder.id), source: 'account', id: folder.id, name: folder.name }));
}

export function localFolderViews(folders: readonly LocalFolder[]): FolderView[] {
  return folders.map((folder) => ({
    key: localFolderKey(folder.id),
    source: 'local',
    id: folder.id,
    name: folder.name,
  }));
}

export function allFolderViews(accountFolders: readonly Folder[], localFolders: readonly LocalFolder[]): FolderView[] {
  return [...accountFolderViews(accountFolders), ...localFolderViews(localFolders)];
}

function accountFolderOf(folders: readonly Folder[]): Map<string, string> {
  const owner = new Map<string, string>();
  for (const folder of folders) {
    for (const endpointId of folder.endpoint_ids) {
      if (!owner.has(endpointId)) {
        owner.set(endpointId, accountFolderKey(folder.id));
      }
    }
  }
  return owner;
}

function byRecentActivity(left: OrganizedEndpoint, right: OrganizedEndpoint): number {
  return right.lastActivity - left.lastActivity;
}

export function buildOrganized(input: OrganizeInput): OrganizedEndpoint[] {
  const folderOfAccount = accountFolderOf(input.accountFolders);
  const localFolderIds = new Set(input.organize.folders.map((folder) => folder.id));
  const placements = new Map(input.organize.placements.map((placement) => [placement.id, placement]));
  const accountRows: OrganizedEndpoint[] = input.account.map((endpoint) => ({
    id: endpoint.id,
    source: endpoint.my_role === 'owner' ? 'account' : 'shared',
    role: endpoint.my_role,
    label: endpoint.title,
    folderKey: folderOfAccount.get(endpoint.id) ?? null,
    tags: endpoint.tags,
    captureCount: endpoint.capture_count,
    lastActivity: Date.parse(endpoint.last_active_at) || 0,
  }));
  const localRows: OrganizedEndpoint[] = input.local
    .filter((entry) => !input.knownAccountIds.has(entry.id))
    .map((entry) => {
      const placement = placements.get(entry.id);
      const folderId = placement?.folderId ?? null;
      return {
        id: entry.id,
        source: 'local',
        role: null,
        label: entry.label,
        folderKey: folderId !== null && localFolderIds.has(folderId) ? localFolderKey(folderId) : null,
        tags: placement?.tags ?? [],
        captureCount: null,
        lastActivity: entry.lastOpened,
      };
    });
  return [...accountRows, ...localRows].sort(byRecentActivity);
}

export function groupByFolder(rows: readonly OrganizedEndpoint[], folders: readonly FolderView[]): RailGroup[] {
  const groups: RailGroup[] = folders.map((folder) => ({
    folder,
    rows: rows.filter((row) => row.folderKey === folder.key),
  }));
  const unfiled = rows.filter((row) => row.folderKey === null);
  return unfiled.length === 0 ? groups : [...groups, { folder: null, rows: unfiled }];
}

export interface LabelCount {
  label: string;
  count: number;
}

export function labelCounts(rows: readonly OrganizedEndpoint[]): LabelCount[] {
  const counts = new Map<string, number>();
  for (const row of rows) {
    for (const tag of row.tags) {
      counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
  }
  return [...counts.entries()]
    .map(([label, count]) => ({ label, count }))
    .sort((left, right) => right.count - left.count || left.label.localeCompare(right.label));
}

function matchesSource(row: OrganizedEndpoint, source: SourceFilter | undefined): boolean {
  return source === undefined || row.source === source;
}

function matchesFolder(row: OrganizedEndpoint, folder: string | undefined): boolean {
  if (folder === undefined) {
    return true;
  }
  return folder === UNFILED_FILTER ? row.folderKey === null : row.folderKey === folder;
}

function matchesLabel(row: OrganizedEndpoint, label: string | undefined): boolean {
  return label === undefined || row.tags.includes(label);
}

export function filterOrganized(rows: readonly OrganizedEndpoint[], filter: OrganizeFilter): OrganizedEndpoint[] {
  return rows.filter(
    (row) => matchesSource(row, filter.source) && matchesFolder(row, filter.folder) && matchesLabel(row, filter.label),
  );
}
