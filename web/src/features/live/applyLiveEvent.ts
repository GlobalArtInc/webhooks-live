import type { QueryClient, QueryKey } from '@tanstack/react-query';
import {
  addCaptureToEndpoint,
  clearCapturePages,
  clearEndpointCaptures,
  mergeCaptureIntoPages,
  queryKeys,
  removeCaptureFromPages,
  type CaptureListData,
} from '../../api/queries';
import type {
  Capture,
  CaptureOrder,
  CapturePage,
  CaptureSummary,
  Endpoint,
  ForwardJob,
  ForwardState,
} from '../../api/types';
import type { RuleRun } from '../../api/ruleRunTypes';
import { updateRuleRunLists } from '../rules/runlogs/api';
import { withRuleRun } from '../rules/runlogs/model';
import { captureKindOf } from '../kinds/kinds';
import { withCaptureCount } from '../list/cache';
import { filtersOfKey, hasActiveFilters, NO_FILTERS, sameActiveFilters, type ActiveFilters } from '../list/listFilters';
import { matchesClientFilter } from '../filterbuilder/refine';
import { flattenCaptures, restoreCaptureInPages } from '../list/listModel';
import type { LiveEvent, LiveReadyPayload } from './types';

export type CatchUpReason = 'ready' | 'resync' | 'invalid';

export type LiveEffect =
  | { kind: 'inserted'; capture: CaptureSummary }
  | { kind: 'hidden'; capture: CaptureSummary }
  | { kind: 'verify'; capture: CaptureSummary }
  | { kind: 'removed'; captureId: string }
  | { kind: 'cleared' }
  | { kind: 'catchUp'; reason: CatchUpReason; ready: LiveReadyPayload | null };

export interface LiveCacheState {
  pages: CaptureListData | undefined;
  order: CaptureOrder;
  endpoint: Endpoint | undefined;
  detail: Capture | undefined;
  filters?: ActiveFilters;
}

export interface LiveOutcome extends LiveCacheState {
  effects: LiveEffect[];
}

function matchesStatusClass(override: number | null, statusClass: string): boolean {
  if (statusClass === 'none') {
    return override === null;
  }
  return override !== null && statusClass === `${Math.floor(override / 100)}xx`;
}

export function matchesActiveFilter(filters: ActiveFilters, summary: CaptureSummary): boolean {
  if (filters.starred !== undefined && (summary.starred ?? false) !== filters.starred) {
    return false;
  }
  if (!matchesClientFilter(summary, filters)) {
    return false;
  }
  if (
    filters.method !== undefined &&
    !filters.method.split(',').some((method) => method.toUpperCase() === summary.method.toUpperCase())
  ) {
    return false;
  }
  if (filters.kind !== undefined && !filters.kind.split(',').includes(captureKindOf(summary))) {
    return false;
  }
  if (
    filters.status !== undefined &&
    !filters.status.split(',').some((statusClass) => matchesStatusClass(summary.status_override, statusClass))
  ) {
    return false;
  }
  const receivedAt = Date.parse(summary.received_at);
  if (filters.since !== undefined && receivedAt < Date.parse(filters.since)) {
    return false;
  }
  return filters.until === undefined || receivedAt < Date.parse(filters.until);
}

type Placement = 'inserted' | 'counted' | 'duplicate';

interface Placed {
  data: CaptureListData;
  placement: Placement;
}

interface Removal {
  data: CaptureListData;
  counted: boolean;
  found: CaptureSummary | null;
}

const FORWARD_RANK: Readonly<Record<ForwardState, number>> = { succeeded: 1, pending: 2, failed: 3 };

function containsCapture(data: CaptureListData, captureId: string): boolean {
  return data.pages.some((page) => page.items.some((item) => item.id === captureId));
}

function lowestSeq(data: CaptureListData): number | null {
  const items = data.pages.at(-1)?.items ?? [];
  return items.at(-1)?.seq ?? null;
}

function hasOlderPages(data: CaptureListData): boolean {
  return (data.pages.at(-1)?.next_cursor ?? null) !== null;
}

function bumpTotals(data: CaptureListData, delta: number): CaptureListData {
  return { ...data, pages: data.pages.map((page) => ({ ...page, total: Math.max(0, page.total + delta) })) };
}

function trimTail(pages: readonly CapturePage[], drop: number): CapturePage[] {
  let remaining = drop;
  const trimmed = [...pages];
  for (let index = trimmed.length - 1; index >= 0 && remaining > 0; index -= 1) {
    const page = trimmed[index];
    if (page === undefined) {
      continue;
    }
    const removed = Math.min(remaining, page.items.length);
    trimmed[index] = { ...page, items: page.items.slice(0, page.items.length - removed) };
    remaining -= removed;
  }
  return trimmed;
}

function clampToCapacity(data: CaptureListData, capacity: number | undefined): CaptureListData {
  const total = data.pages[0]?.total ?? 0;
  if (capacity === undefined || total <= capacity) {
    return data;
  }
  const clamped = data.pages.map((page) => ({ ...page, total: capacity }));
  if (hasOlderPages(data)) {
    return { ...data, pages: clamped };
  }
  const held = clamped.reduce((sum, page) => sum + page.items.length, 0);
  return { ...data, pages: held > capacity ? trimTail(clamped, held - capacity) : clamped };
}

function placeCapture(
  data: CaptureListData,
  capture: CaptureSummary,
  order: CaptureOrder,
  capacity: number | undefined,
): Placed {
  if (containsCapture(data, capture.id)) {
    return { data, placement: 'duplicate' };
  }
  if (order === 'asc') {
    return { data: mergeCaptureIntoPages(data, capture, order), placement: 'inserted' };
  }
  const lowest = lowestSeq(data);
  if (lowest !== null && capture.seq < lowest && hasOlderPages(data)) {
    return { data: clampToCapacity(bumpTotals(data, 1), capacity), placement: 'counted' };
  }
  return { data: clampToCapacity(restoreCaptureInPages(data, capture), capacity), placement: 'inserted' };
}

function removeCapture(data: CaptureListData, captureId: string, seq: number): Removal {
  const found = flattenCaptures(data).find((item) => item.id === captureId) ?? null;
  if (found !== null) {
    return { data: removeCaptureFromPages(data, captureId), counted: true, found };
  }
  const lowest = lowestSeq(data);
  if (lowest !== null && seq < lowest && hasOlderPages(data)) {
    return { data: bumpTotals(data, -1), counted: true, found: null };
  }
  return { data, counted: false, found: null };
}

function forwardStateOfJob(job: ForwardJob): ForwardState {
  return job.state === 'running' ? 'pending' : job.state;
}

export function worstForwardState(states: readonly ForwardState[]): ForwardState | null {
  return states.reduce<ForwardState | null>(
    (worst, state) => (worst === null || FORWARD_RANK[state] > FORWARD_RANK[worst] ? state : worst),
    null,
  );
}

function mergeForwardState(existing: ForwardState | null, job: ForwardJob): ForwardState {
  const mapped = forwardStateOfJob(job);
  if (existing === null || (existing === 'pending' && mapped !== 'pending')) {
    return mapped;
  }
  return FORWARD_RANK[mapped] > FORWARD_RANK[existing] ? mapped : existing;
}

function upsertJob(jobs: readonly ForwardJob[], job: ForwardJob): ForwardJob[] {
  return jobs.some((existing) => existing.id === job.id)
    ? jobs.map((existing) => (existing.id === job.id ? job : existing))
    : [job, ...jobs];
}

function withForwardState(data: CaptureListData, captureId: string, state: ForwardState | null): CaptureListData {
  if (!containsCapture(data, captureId)) {
    return data;
  }
  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.map((item) => (item.id === captureId ? { ...item, forward_state: state } : item)),
    })),
  };
}

function countedBytes(before: Endpoint, after: Endpoint, capture: CaptureSummary): Endpoint {
  return after.last_seq === before.last_seq ? after : { ...after, bytes_used: after.bytes_used + capture.body_size };
}

function applyCapture(state: LiveCacheState, capture: CaptureSummary): LiveOutcome {
  const { pages, endpoint, order } = state;
  const filters = state.filters ?? NO_FILTERS;
  const capacity = endpoint?.max_captures;
  const fresh = endpoint === undefined || capture.seq > endpoint.last_seq;
  let nextPages = pages;
  let effect: LiveEffect | null = null;
  if (!matchesActiveFilter(filters, capture)) {
    effect = fresh ? { kind: 'hidden', capture } : null;
  } else if (filters.q !== undefined) {
    effect = fresh ? { kind: 'verify', capture } : null;
  } else if (pages === undefined) {
    effect = fresh ? { kind: 'inserted', capture } : null;
  } else {
    const placed = placeCapture(pages, capture, order, capacity);
    nextPages = placed.data;
    effect = placed.placement === 'inserted' ? { kind: 'inserted', capture } : null;
  }
  const nextEndpoint =
    endpoint === undefined ? undefined : countedBytes(endpoint, addCaptureToEndpoint(endpoint, capture), capture);
  return { ...state, pages: nextPages, endpoint: nextEndpoint, effects: effect === null ? [] : [effect] };
}

function applyDeleted(state: LiveCacheState, captureId: string, seq: number): LiveOutcome {
  const { pages, endpoint } = state;
  if (pages === undefined) {
    const nextEndpoint = endpoint === undefined ? undefined : withCaptureCount(endpoint, -1);
    return { ...state, endpoint: nextEndpoint, effects: [{ kind: 'removed', captureId }] };
  }
  const removal = removeCapture(pages, captureId, seq);
  if (!removal.counted) {
    return { ...state, effects: [] };
  }
  const counted = endpoint === undefined ? undefined : withCaptureCount(endpoint, -1);
  const nextEndpoint =
    counted !== undefined && removal.found !== null
      ? { ...counted, bytes_used: Math.max(0, counted.bytes_used - removal.found.body_size) }
      : counted;
  return { ...state, pages: removal.data, endpoint: nextEndpoint, effects: [{ kind: 'removed', captureId }] };
}

function applyForwardResult(state: LiveCacheState, captureId: string, job: ForwardJob): LiveOutcome {
  const detail = state.detail?.id === captureId ? state.detail : undefined;
  const forwards = detail === undefined ? null : upsertJob(detail.forwards, job);
  const aggregate = forwards === null ? null : worstForwardState(forwards.map(forwardStateOfJob));
  const existing =
    state.pages === undefined ? null : flattenCaptures(state.pages).find((item) => item.id === captureId);
  const listState = forwards === null ? mergeForwardState(existing?.forward_state ?? null, job) : aggregate;
  return {
    ...state,
    pages: state.pages === undefined ? undefined : withForwardState(state.pages, captureId, listState),
    detail:
      detail === undefined || forwards === null ? state.detail : { ...detail, forwards, forward_state: aggregate },
    effects: [],
  };
}

function applyRuleResult(state: LiveCacheState, captureId: string, run: RuleRun): LiveOutcome {
  const detail = state.detail?.id === captureId ? state.detail : undefined;
  return { ...state, detail: detail === undefined ? state.detail : withRuleRun(detail, run), effects: [] };
}

export function applyLiveEvent(state: LiveCacheState, event: LiveEvent): LiveOutcome {
  switch (event.name) {
    case 'capture':
      return applyCapture(state, event.data);
    case 'capture_deleted':
      return applyDeleted(state, event.data.capture_id, event.data.seq);
    case 'captures_cleared':
      return {
        ...state,
        pages: state.pages === undefined ? undefined : clearCapturePages(),
        endpoint: state.endpoint === undefined ? undefined : clearEndpointCaptures(state.endpoint),
        effects: [{ kind: 'cleared' }],
      };
    case 'endpoint_updated':
      return { ...state, endpoint: event.data, effects: [] };
    case 'forward_result':
      return applyForwardResult(state, event.data.capture_id, event.data.job);
    case 'rule_result':
      return applyRuleResult(state, event.data.capture_id, event.data.run);
    case 'ready':
      return { ...state, effects: [{ kind: 'catchUp', reason: 'ready', ready: event.data }] };
    case 'resync':
      return { ...state, effects: [{ kind: 'catchUp', reason: 'resync', ready: null }] };
    case 'endpoint_deleted':
    case 'bye':
      return { ...state, effects: [] };
  }
}

function orderOfKey(queryKey: QueryKey): CaptureOrder {
  const filters = queryKey[3];
  return typeof filters === 'object' && filters !== null && 'order' in filters && filters.order === 'asc'
    ? 'asc'
    : 'desc';
}

function belongsToEndpoint(event: LiveEvent, endpointId: string): boolean {
  switch (event.name) {
    case 'capture':
      return event.data.endpoint_id === endpointId;
    case 'endpoint_updated':
      return event.data.id === endpointId;
    case 'ready':
      return event.data.endpoint_id === endpointId;
    default:
      return true;
  }
}

function dropDetails(queryClient: QueryClient, endpointId: string, captureId: string | null): void {
  const queryKey = captureId === null ? queryKeys.captureDetails(endpointId) : queryKeys.capture(endpointId, captureId);
  queryClient.removeQueries({ queryKey, type: 'inactive' });
  void queryClient.invalidateQueries({ queryKey, refetchType: 'active' });
}

export function applyLiveEventToCache(
  queryClient: QueryClient,
  endpointId: string,
  event: LiveEvent,
  active: ActiveFilters = NO_FILTERS,
): LiveEffect[] {
  if (!belongsToEndpoint(event, endpointId)) {
    return [];
  }
  const endpoint = queryClient.getQueryData<Endpoint>(queryKeys.endpoint(endpointId));
  const detailId = event.name === 'forward_result' || event.name === 'rule_result' ? event.data.capture_id : null;
  const detail =
    detailId === null ? undefined : queryClient.getQueryData<Capture>(queryKeys.capture(endpointId, detailId));
  const lists = queryClient.getQueryCache().findAll({ queryKey: queryKeys.captureLists(endpointId) });
  let shown: LiveEffect[] | null = null;
  let basis: LiveOutcome | null = null;
  for (const query of lists) {
    const pages = queryClient.getQueryData<CaptureListData>(query.queryKey);
    const filters = filtersOfKey(query.queryKey);
    const outcome = applyLiveEvent({ pages, order: orderOfKey(query.queryKey), endpoint, detail, filters }, event);
    if (outcome.pages !== undefined && outcome.pages !== pages) {
      queryClient.setQueryData(query.queryKey, outcome.pages);
    }
    if (sameActiveFilters(filters, active)) {
      shown = outcome.effects;
    }
    if (basis === null || !hasActiveFilters(filters)) {
      basis = outcome;
    }
  }
  const unbound = applyLiveEvent({ pages: undefined, order: 'desc', endpoint, detail, filters: active }, event);
  const outcome = basis ?? unbound;
  const effects = shown ?? unbound.effects;
  if (outcome.endpoint !== undefined && outcome.endpoint !== endpoint) {
    queryClient.setQueryData(queryKeys.endpoint(endpointId), outcome.endpoint);
  }
  if (outcome.detail !== undefined && outcome.detail !== detail && detailId !== null) {
    queryClient.setQueryData(queryKeys.capture(endpointId, detailId), outcome.detail);
  }
  if (event.name === 'rule_result') {
    updateRuleRunLists(queryClient, endpointId, event.data.run);
  }
  if (event.name === 'endpoint_updated' || event.name === 'endpoint_deleted') {
    void queryClient.invalidateQueries({ queryKey: queryKeys.endpointLists() });
  }
  for (const effect of effects) {
    if (effect.kind === 'removed') {
      dropDetails(queryClient, endpointId, effect.captureId);
    } else if (effect.kind === 'cleared') {
      dropDetails(queryClient, endpointId, null);
    }
  }
  return effects;
}
