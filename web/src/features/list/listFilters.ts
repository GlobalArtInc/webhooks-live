import type { QueryKey } from '@tanstack/react-query';
import type { CaptureListFilters } from '../../api/types';

export type ActiveFilters = Pick<CaptureListFilters, 'method' | 'kind' | 'status' | 'q' | 'since' | 'until'>;

export const NO_FILTERS: ActiveFilters = {};

const FILTER_FIELDS = ['method', 'kind', 'status', 'q', 'since', 'until'] as const;

export function activeFiltersOf(filters: Partial<CaptureListFilters>): ActiveFilters {
  const active: ActiveFilters = {};
  if (filters.method) {
    active.method = filters.method;
  }
  if (filters.kind) {
    active.kind = filters.kind;
  }
  if (filters.status) {
    active.status = filters.status;
  }
  if (filters.q) {
    active.q = filters.q;
  }
  if (filters.since) {
    active.since = filters.since;
  }
  if (filters.until) {
    active.until = filters.until;
  }
  return active;
}

export function filtersOfKey(queryKey: QueryKey): ActiveFilters {
  const filters: unknown = queryKey[3];
  return typeof filters === 'object' && filters !== null ? activeFiltersOf(filters) : NO_FILTERS;
}

export function hasActiveFilters(filters: ActiveFilters): boolean {
  return FILTER_FIELDS.some((field) => filters[field] !== undefined);
}

export function sameActiveFilters(left: ActiveFilters, right: ActiveFilters): boolean {
  return FILTER_FIELDS.every((field) => left[field] === right[field]);
}

export function activeFiltersKey(filters: ActiveFilters): string {
  return JSON.stringify(FILTER_FIELDS.map((field) => filters[field] ?? null));
}
