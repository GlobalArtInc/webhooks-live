import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import {
  claimEndpoint,
  createFolder,
  deleteEndpoint,
  deleteFolder,
  deleteFolderEndpoint,
  listEndpoints,
  listFolders,
  patchEndpoint,
  patchFolder,
  putFolderEndpoint,
} from '../../api/client';
import { queryKeys, shouldRetryRequest } from '../../api/queries';
import type { Endpoint, EndpointListQuery, Folder } from '../../api/types';
import { forgetEndpoint } from '../endpoints/store';
import { forgetLocalEndpointOrganize } from './organizeStore';

const ENDPOINT_PAGE_LIMIT = 200;
const MAX_ENDPOINT_PAGES = 50;

export function folderQueryKey() {
  return ['folders'] as const;
}

export function accountEndpointsQueryKey(query: EndpointListQuery) {
  return [...queryKeys.endpointLists(), 'account', query] as const;
}

async function loadFolders(): Promise<Folder[]> {
  const page = await listFolders();
  return page.items;
}

async function listAllEndpoints(query: EndpointListQuery): Promise<Endpoint[]> {
  const items: Endpoint[] = [];
  let cursor: string | null = null;
  for (let page = 0; page < MAX_ENDPOINT_PAGES; page += 1) {
    const result = await listEndpoints({ ...query, cursor, limit: ENDPOINT_PAGE_LIMIT });
    items.push(...result.items);
    if (result.next_cursor === null) {
      break;
    }
    cursor = result.next_cursor;
  }
  return items;
}

async function accountFolderNamed(queryClient: QueryClient, name: string): Promise<Folder> {
  const folders = await queryClient.fetchQuery({ queryKey: folderQueryKey(), queryFn: loadFolders });
  const existing = folders.find((folder) => folder.name === name);
  return existing ?? createFolder(name);
}

function invalidateFolders(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: folderQueryKey() });
}

function invalidateAccount(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: queryKeys.endpointLists() });
  invalidateFolders(queryClient);
}

export function useAccountFolders(enabled: boolean) {
  return useQuery({
    queryKey: folderQueryKey(),
    queryFn: loadFolders,
    enabled,
    retry: shouldRetryRequest,
  });
}

export function useAccountEndpoints(query: EndpointListQuery, enabled: boolean) {
  return useQuery({
    queryKey: accountEndpointsQueryKey(query),
    queryFn: () => listAllEndpoints(query),
    enabled,
    retry: shouldRetryRequest,
  });
}

export function useCreateAccountFolder() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => createFolder(name),
    onSuccess: () => invalidateFolders(queryClient),
  });
}

export interface RenameFolderVariables {
  folderId: string;
  name: string;
}

export function useRenameAccountFolder() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ folderId, name }: RenameFolderVariables) => patchFolder(folderId, { name }),
    onSuccess: () => invalidateFolders(queryClient),
  });
}

export function useDeleteAccountFolder() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (folderId: string) => deleteFolder(folderId),
    onSuccess: () => invalidateAccount(queryClient),
  });
}

export interface MoveAccountEndpointVariables {
  endpointId: string;
  fromFolderId: string | null;
  toFolderId: string | null;
}

export function useMoveAccountEndpoint() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ endpointId, fromFolderId, toFolderId }: MoveAccountEndpointVariables) => {
      if (fromFolderId !== null) {
        await deleteFolderEndpoint(fromFolderId, endpointId);
      }
      if (toFolderId !== null) {
        await putFolderEndpoint(toFolderId, endpointId);
      }
    },
    onSuccess: () => invalidateFolders(queryClient),
  });
}

export interface MoveToAccountVariables {
  endpointId: string;
  label: string | null;
  tags: readonly string[];
  folderName: string | null;
}

export interface MoveToAccountResult {
  detailsSaved: boolean;
}

export function useDeleteOwnedEndpoint(onDeleted: (endpointId: string) => void) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (endpointId: string) => deleteEndpoint(endpointId),
    onSuccess: (_result, endpointId) => {
      queryClient.removeQueries({ queryKey: queryKeys.endpoint(endpointId) });
      queryClient.removeQueries({ queryKey: queryKeys.captures(endpointId) });
      invalidateAccount(queryClient);
      onDeleted(endpointId);
    },
  });
}

export function useMoveToAccount(onMoved: (result: MoveToAccountResult) => void) {
  const queryClient = useQueryClient();
  return useMutation<MoveToAccountResult, Error, MoveToAccountVariables>({
    mutationFn: async (variables) => {
      await claimEndpoint(variables.endpointId);
      try {
        await patchEndpoint(variables.endpointId, { title: variables.label, tags: [...variables.tags] });
        if (variables.folderName !== null) {
          const folder = await accountFolderNamed(queryClient, variables.folderName);
          await putFolderEndpoint(folder.id, variables.endpointId);
        }
        return { detailsSaved: true };
      } catch {
        return { detailsSaved: false };
      }
    },
    onSuccess: (result, variables) => {
      forgetEndpoint(variables.endpointId);
      forgetLocalEndpointOrganize(variables.endpointId);
      invalidateAccount(queryClient);
      onMoved(result);
    },
  });
}
