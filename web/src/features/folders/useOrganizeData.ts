import { useMemo } from 'react';
import { useInstance } from '../../api/queries';
import type { Endpoint, EndpointListQuery, Folder } from '../../api/types';
import { useMyEndpoints } from '../endpoints/store';
import { useCachedEndpointTitles } from '../endpoints/titleCache';
import { useAccountEndpoints, useAccountFolders } from './api';
import type { LocalOrganize } from './localOrganize';
import { allFolderViews, buildOrganized, type FolderView, type OrganizedEndpoint } from './model';
import { useLocalOrganize } from './organizeStore';

export interface SessionState {
  ready: boolean;
  signedIn: boolean;
  signInAvailable: boolean;
}

export interface OrganizeData {
  session: SessionState;
  allRows: OrganizedEndpoint[];
  shownRows: OrganizedEndpoint[];
  folders: FolderView[];
  accountFolders: readonly Folder[];
  organize: LocalOrganize;
  pending: boolean;
  failed: boolean;
}

const NO_FOLDERS: readonly Folder[] = [];
const NO_ENDPOINTS: readonly Endpoint[] = [];

export function useSession(): SessionState {
  const instance = useInstance();
  const data = instance.data;
  return {
    ready: data !== undefined,
    signedIn: data !== undefined && data.user !== null,
    signInAvailable: data !== undefined && data.auth.sign_in_available,
  };
}

export function useOrganizeData(serverQuery: EndpointListQuery): OrganizeData {
  const session = useSession();
  const stored = useMyEndpoints();
  const titles = useCachedEndpointTitles(stored.map((entry) => entry.id));
  const local = useMemo(
    () => stored.map((entry, index) => ({ ...entry, label: entry.label ?? titles[index] ?? entry.title ?? null })),
    [stored, titles],
  );
  const organize = useLocalOrganize();
  const everyAccount = useAccountEndpoints({}, session.signedIn);
  const shown = useAccountEndpoints(serverQuery, session.signedIn);
  const folders = useAccountFolders(session.signedIn);
  const accountFolders = folders.data ?? NO_FOLDERS;
  const allAccount = everyAccount.data ?? NO_ENDPOINTS;
  const knownAccountIds = new Set(allAccount.map((endpoint) => endpoint.id));
  const shared = { local, organize, accountFolders, knownAccountIds };
  return {
    session,
    allRows: buildOrganized({ ...shared, account: allAccount }),
    shownRows: buildOrganized({ ...shared, account: shown.data ?? NO_ENDPOINTS }),
    folders: allFolderViews(accountFolders, organize.folders),
    accountFolders,
    organize,
    pending: session.signedIn && (everyAccount.isPending || folders.isPending),
    failed: everyAccount.isError || shown.isError || folders.isError,
  };
}
