import type { EndpointListQuery } from '../../api/types';
import { isValidTag } from './localOrganize';
import { folderIdFromKey, UNFILED_FILTER, type SourceFilter } from './model';

const SOURCE_FILTERS: readonly SourceFilter[] = ['local', 'account', 'shared'];
const FOLDER_FILTER_PATTERN = /^(local|account):[a-z0-9-]{1,64}$/;
const ACCOUNT_PREFIX = 'account:';

export interface EndpointsSearch {
  folder?: string;
  label?: string;
  source?: SourceFilter;
}

export function serverQueryFor(search: EndpointsSearch): EndpointListQuery {
  const folder = search.folder?.startsWith(ACCOUNT_PREFIX) ? folderIdFromKey(search.folder) : undefined;
  return { tag: search.label, folder };
}

function isSourceFilter(value: unknown): value is SourceFilter {
  return SOURCE_FILTERS.some((source) => source === value);
}

export function validateEndpointsSearch(search: Record<string, unknown>): EndpointsSearch {
  const result: EndpointsSearch = {};
  const { folder, label, source } = search;
  if (typeof folder === 'string' && (folder === UNFILED_FILTER || FOLDER_FILTER_PATTERN.test(folder))) {
    result.folder = folder;
  }
  if (typeof label === 'string' && isValidTag(label)) {
    result.label = label;
  }
  if (isSourceFilter(source)) {
    result.source = source;
  }
  return result;
}
