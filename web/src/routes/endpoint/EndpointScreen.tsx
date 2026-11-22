import { Outlet, useMatchRoute, useNavigate, useParams } from '@tanstack/react-router';
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useClearCaptures, useEndpoint } from '../../api/queries';
import { CaptureList, type ListNavigation } from '../../components/CaptureList/CaptureList';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { Announcer } from '../../components/Announcer/Announcer';
import { copyText } from '../../components/CopyButton/clipboard';
import { ConnectionIndicator } from '../../components/ConnectionIndicator/ConnectionIndicator';
import { EmptyState } from '../../components/EmptyState/EmptyState';
import { EndpointHeader } from '../../components/EndpointHeader/EndpointHeader';
import { EmptySamples } from '../../features/kinds/samples/EmptySamples';
import { FilterBar } from '../../components/FilterBar/FilterBar';
import { ListPager, type ListPagerProps } from '../../components/ListPager/ListPager';
import { SelectionBar } from '../../components/SelectionBar/SelectionBar';
import { SplitHandle } from '../../components/SplitHandle/SplitHandle';
import { StatePage } from '../../components/StatePage/StatePage';
import { UndoToast } from '../../components/UndoToast/UndoToast';
import { useMarkedCapture } from '../../features/diff';
import { RESIZE_STEP_PX } from '../../features/endpoint/constants';
import { sampleCurl } from '../../features/endpoint/sampleCurl';
import { useEndpointSelection } from '../../features/endpoint/useSelection';
import { listFiltersOf } from '../../features/filters/captureSearch';
import { hasRefinement, refineCaptures } from '../../features/filterbuilder';
import { useCaptureSearch } from '../../features/filters/useCaptureSearch';
import { useViewportLayout } from '../../features/endpoint/useViewport';
import { markEndpointOpened } from '../../features/endpoints';
import {
  isFollowing,
  LiveToolbar,
  reduceFollow,
  saveLivePrefs,
  type FollowAction,
  useFollowControls,
  useLastSeen,
  useLiveEndpoint,
  useLivePrefs,
  useTabBadge,
} from '../../features/live';
import {
  activateCompare,
  activeFiltersOf,
  hasActiveFilters,
  LIST_WIDTH_MAX,
  LIST_WIDTH_MIN,
  loadListWidth,
  neighbourOf,
  saveListWidth,
  type CaptureListModel,
  useCaptureList,
  useDeleteWithUndo,
  useVisibleMinuteNow,
} from '../../features/list';
import { useShortcutHandlers, useShortcutScope } from '../../features/shortcuts';
import { useBulkDelete, useSelectMode } from '../../features/selectmode';
import { selectionFitsExport, type ExportScope } from '../../features/exportdialog/exportRequest';
import styles from './EndpointScreen.module.css';

const ENDPOINT_ROUTE_ID = '/shell/e/$id';

const CapturePane = lazy(() =>
  import('../../features/endpoint/CapturePane').then((module) => ({ default: module.CapturePane })),
);

const ExportDialog = lazy(() =>
  import('../../features/exportdialog/ExportDialog').then((module) => ({ default: module.ExportDialog })),
);

function PaneLoading() {
  const { t } = useTranslation();
  return (
    <div role="status" className={styles.paneLoading}>
      {t('wl.detail.state.loading')}
    </div>
  );
}

export function EndpointScreen() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { id: endpointId } = useParams({ from: ENDPOINT_ROUTE_ID });
  const endpointQuery = useEndpoint(endpointId);
  const endpoint = endpointQuery.data;
  const loaded = endpoint !== undefined;
  const urlSearch = useCaptureSearch();
  const filters = useMemo(() => listFiltersOf(urlSearch.search), [urlSearch.search]);
  const refining = hasRefinement(urlSearch.search);
  const filtered = hasActiveFilters(filters) || refining;
  const captureList = useCaptureList(endpointId, filters);
  const shownItems = useMemo(
    () => refineCaptures(captureList.items, urlSearch.search),
    [captureList.items, urlSearch.search],
  );
  const list: CaptureListModel = refining ? { ...captureList, items: shownItems, total: null } : captureList;
  const liveFilters = useMemo(
    () => activeFiltersOf({ ...filters, ip: urlSearch.search.ip, path: urlSearch.search.path }),
    [filters, urlSearch.search.ip, urlSearch.search.path],
  );
  const layout = useViewportLayout();
  const narrow = layout === 'narrow';
  const wide = layout === 'wide';
  const follow = useFollowControls();
  const livePrefs = useLivePrefs();
  const [listAtTop, setListAtTop] = useState(true);
  const newestId = list.items[0]?.id ?? null;
  const selection = useEndpointSelection(endpointId, list, !narrow, follow.following, (captureId) => {
    follow.dispatch({ kind: 'select', captureId, newestId });
  });
  const live = useLiveEndpoint(endpointId, {
    following: follow.following,
    selectedId: selection.paneId,
    onFollowNewest: (captureId) => {
      if (selection.captureId !== null) {
        selection.replace(captureId);
      }
    },
    filters: liveFilters,
  });
  const matchRoute = useMatchRoute();
  const workspaceOpen = matchRoute({ to: '/e/$id/settings', params: { id: endpointId }, fuzzy: true }) !== false;
  const deletion = useDeleteWithUndo(endpointId);
  const bulk = useBulkDelete(endpointId);
  const selectMode = useSelectMode(endpointId);
  const checkedCaptures = selectMode.checkedIn(list.items);
  const clear = useClearCaptures(endpointId);
  const marked = useMarkedCapture(endpointId);
  const now = useVisibleMinuteNow();
  const [listWidth, setListWidth] = useState(loadListWidth);
  const [closedFor, setClosedFor] = useState<string | null>(null);
  const [clearOpen, setClearOpen] = useState(false);
  const [clearFailed, setClearFailed] = useState(false);
  const [bulkConfirmOpen, setBulkConfirmOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportScope, setExportScope] = useState<ExportScope>('all');
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const backRequested = useRef(false);

  useTabBadge(live.awayCount);
  useLastSeen(endpointId, newestId, listAtTop);

  useEffect(() => {
    if (loaded) {
      markEndpointOpened(endpointId);
    }
  }, [endpointId, loaded]);

  useEffect(() => {
    if (backRequested.current && selection.captureId === null) {
      backRequested.current = false;
      listRef.current?.querySelector<HTMLElement>('[role="listbox"]')?.focus();
    }
  }, [selection.captureId]);

  const paneId = selection.paneId;
  const paneHidden = layout === 'medium' && closedFor !== null && closedFor === paneId;
  const paneTarget = narrow ? selection.captureId : paneHidden ? null : paneId;
  const shortcutsActive = endpoint !== undefined && live.endReason === null;

  useShortcutScope('endpoint', shortcutsActive);
  useShortcutScope('capture', shortcutsActive && paneTarget !== null);
  useShortcutHandlers({
    next: selection.next,
    previous: selection.previous,
    newest: goNewest,
    oldest: selection.last,
    toggleFollow: () => {
      applyFollow({ kind: 'toggle' });
    },
    search: () => {
      searchRef.current?.focus();
    },
    copyUrl: copyCaptureUrl,
    settings: () => {
      void navigate({ to: '/e/$id/settings', params: { id: endpointId } });
    },
    forwarding: () => {
      void navigate({ to: '/e/$id/settings/$section', params: { id: endpointId, section: 'forwarding' } });
    },
    compare: () => {
      if (paneTarget !== null) {
        compare(paneTarget);
      }
    },
    delete: () => {
      if (paneTarget !== null) {
        requestDelete(paneTarget);
      }
    },
    close: narrow && paneTarget !== null ? goBack : undefined,
  });

  if (endpoint === undefined) {
    return null;
  }

  if (live.endReason === 'locked') {
    return null;
  }

  if (live.endReason !== null) {
    return <StatePage kind={live.endReason === 'missing' ? 'not-found' : 'gone'} />;
  }

  const showEmpty = !filtered && list.hasData && list.total === 0 && list.items.length === 0;
  const noMatch =
    filtered && list.hasData && list.items.length === 0 && (refining ? !list.hasNextPage : list.total === 0);
  const curl = sampleCurl(endpoint.url);

  const pagerProps: ListPagerProps = {
    position: selection.position,
    total: list.total,
    onFirst: selection.first,
    onPrev: selection.previous,
    onNext: selection.next,
    onLast: selection.last,
  };

  function applyFollow(action: FollowAction): void {
    const next = reduceFollow(follow.state, action);
    follow.dispatch(action);
    if (isFollowing(next) && newestId !== null && selection.captureId !== null && selection.captureId !== newestId) {
      selection.replace(newestId);
    }
  }

  function goNewest(): void {
    if (!follow.following) {
      follow.dispatch({ kind: 'toggle' });
    }
    selection.first();
  }

  function copyCaptureUrl(): void {
    if (endpoint !== undefined) {
      void copyText(endpoint.url);
    }
  }

  function navigateList(navigation: ListNavigation): void {
    switch (navigation) {
      case 'first':
        goNewest();
        return;
      case 'previous':
        selection.previous();
        return;
      case 'next':
        selection.next();
        return;
      case 'last':
        selection.last();
    }
  }

  function requestDelete(captureId: string): void {
    const capture = list.items.find((item) => item.id === captureId);
    if (capture === undefined) {
      return;
    }
    bulk.commit();
    const wasSelected = selection.captureId === captureId;
    const neighbour = neighbourOf(list.items, captureId);
    deletion.schedule(capture, wasSelected);
    if (wasSelected) {
      selection.replace(neighbour);
    }
  }

  function requestBulkDelete(): void {
    if (checkedCaptures.length > 0) {
      setBulkConfirmOpen(true);
    }
  }

  function confirmBulkDelete(): void {
    setBulkConfirmOpen(false);
    deletion.commit();
    bulk.schedule(checkedCaptures);
    selectMode.end();
  }

  function openExport(): void {
    const scope: ExportScope =
      selectMode.active && selectionFitsExport(checkedCaptures.length) ? 'selected' : filtered ? 'filtered' : 'all';
    setExportScope(scope);
    setExportOpen(true);
  }

  function undoBulkDelete(): void {
    bulk.undo();
  }

  function undoDelete(): void {
    const restored = deletion.undo();
    if (restored !== null && restored.wasSelected) {
      selection.replace(restored.capture.id);
    }
  }

  function compare(captureId: string): void {
    const route = activateCompare(endpointId, captureId, marked);
    if (route !== null) {
      void navigate({ to: '/e/$id/diff/$a/$b', params: route });
    }
  }

  function goBack(): void {
    backRequested.current = true;
    selection.replace(null);
  }

  function resizeList(value: number): void {
    setListWidth(value);
  }

  function commitListWidth(value: number): void {
    setListWidth(value);
    saveListWidth(value);
  }

  function clearAll(): void {
    setClearFailed(false);
    clear.mutate(undefined, {
      onSuccess: () => {
        setClearOpen(false);
      },
      onError: () => {
        setClearFailed(true);
      },
    });
  }

  return (
    <section className={styles.screen}>
      <h1 className={styles.srOnly}>{t('wl.screen.endpoint')}</h1>
      <EndpointHeader
        endpointId={endpointId}
        captureUrl={endpoint.url}
        curl={curl}
        count={endpoint.capture_count}
        capacity={endpoint.max_captures}
        expiresAt={endpoint.expires_at}
        now={now}
        narrow={narrow}
        status={<ConnectionIndicator state={live.connection} lastEventAt={live.lastEventAt} now={now} />}
        addresses={endpoint}
        onDeleteAll={() => {
          setClearOpen(true);
        }}
      />
      {deletion.failed ? (
        <p role="alert" className={styles.notice}>
          {t('wl.endpoint.delete.failed')}
        </p>
      ) : null}
      {bulk.failed ? (
        <p role="alert" className={styles.notice}>
          {t('wl.selection.delete.failed')}
        </p>
      ) : null}
      {workspaceOpen ? (
        <div className={styles.workspace}>
          <Outlet />
        </div>
      ) : showEmpty ? (
        <div className={styles.empty}>
          <EmptyState captureUrl={endpoint.url} curl={curl} />
          <EmptySamples endpoint={endpoint} />
        </div>
      ) : (
        <div className={styles.body} data-pane={paneHidden ? 'hidden' : 'shown'}>
          <div ref={listRef} className={styles.list} style={wide ? { width: listWidth } : undefined}>
            <FilterBar
              searchRef={searchRef}
              search={urlSearch.search}
              layout={layout}
              hiddenCount={live.hiddenCount}
              onChange={urlSearch.update}
              onClear={urlSearch.clear}
            />
            {refining && list.hasData ? (
              <p className={styles.noMatch}>{t('wl.filterbuilder.shown', { shown: list.items.length })}</p>
            ) : null}
            {noMatch ? <p className={styles.noMatch}>{t('wl.endpoint.filter.empty')}</p> : null}
            <div className={styles.tools}>
              <LiveToolbar
                follow={follow.state}
                announce={livePrefs.announce}
                onToggleFollow={() => {
                  applyFollow({ kind: 'toggle' });
                }}
                onToggleAnnounce={() => {
                  saveLivePrefs({ announce: !livePrefs.announce });
                }}
              />
              <SelectionBar
                active={selectMode.active}
                count={checkedCaptures.length}
                loadedCount={list.items.length}
                onBegin={selectMode.begin}
                onEnd={selectMode.end}
                onSelectAll={() => {
                  selectMode.selectAll(list.items.map((item) => item.id));
                }}
                onExport={openExport}
                onDelete={requestBulkDelete}
              />
            </div>
            <CaptureList
              selection={{
                active: selectMode.active,
                checkedIds: selectMode.ids,
                onToggle: selectMode.toggle,
                onLongPress: narrow ? selectMode.check : undefined,
              }}
              items={list.items}
              selectedId={paneId}
              markedId={marked}
              variant={narrow ? 'two-line' : 'one-line'}
              now={now}
              unreadIds={live.unreadIds}
              arrivingIds={live.arrivingIds}
              following={follow.following}
              total={list.total}
              pending={list.isPending}
              failed={list.error !== null && !list.hasData}
              pageFailed={list.error !== null && list.hasData}
              hasNextPage={list.hasNextPage}
              isFetchingNextPage={list.isFetchingNextPage}
              onNavigate={navigateList}
              onSelect={selection.open}
              onDelete={requestDelete}
              onCompare={compare}
              onLoadMore={list.fetchNextPage}
              onRetry={list.retry}
              onAtTopChange={setListAtTop}
              onPillActivate={() => {
                applyFollow({ kind: 'resume' });
              }}
            />
            {narrow ? null : <ListPager {...pagerProps} />}
          </div>
          {wide ? (
            <SplitHandle
              value={listWidth}
              min={LIST_WIDTH_MIN}
              max={LIST_WIDTH_MAX}
              step={RESIZE_STEP_PX}
              onChange={resizeList}
              onCommit={commitListWidth}
            />
          ) : null}
          {paneTarget === null ? null : (
            <div className={narrow ? styles.detailNarrow : styles.detail}>
              <Suspense fallback={<PaneLoading />}>
                <CapturePane
                  endpointId={endpointId}
                  captureId={paneTarget}
                  markedId={marked}
                  narrow={narrow}
                  pager={narrow ? pagerProps : null}
                  onBack={narrow ? goBack : undefined}
                  onClose={layout === 'medium' ? () => setClosedFor(paneId) : undefined}
                  onCompare={() => {
                    if (paneTarget !== null) {
                      compare(paneTarget);
                    }
                  }}
                />
              </Suspense>
            </div>
          )}
        </div>
      )}
      {deletion.pending === null ? null : (
        <UndoToast message={t('wl.endpoint.undo.message')} onUndo={undoDelete} onDismiss={deletion.commit} />
      )}
      {bulk.pending === null ? null : (
        <UndoToast
          message={t('wl.selection.undo.message', { count: bulk.pending.length })}
          onUndo={undoBulkDelete}
          onDismiss={bulk.commit}
        />
      )}
      <ConfirmDialog
        open={bulkConfirmOpen}
        title={t('wl.selection.delete.title', { count: checkedCaptures.length })}
        body={t('wl.selection.delete.body')}
        confirmLabel={t('wl.selection.delete.confirm')}
        cancelLabel={t('wl.selection.delete.cancel')}
        busy={false}
        onConfirm={confirmBulkDelete}
        onOpenChange={setBulkConfirmOpen}
      />
      {exportOpen ? (
        <Suspense fallback={null}>
          <ExportDialog
            endpointId={endpointId}
            initialScope={exportScope}
            selectedIds={checkedCaptures.map((item) => item.id)}
            filters={filters}
            counts={{ filtered: list.total, all: endpoint.capture_count }}
            sizes={{
              selected: checkedCaptures.reduce((total, item) => total + item.body_size, 0),
              all: endpoint.bytes_used,
            }}
            onOpenChange={setExportOpen}
          />
        </Suspense>
      ) : null}
      <ConfirmDialog
        open={clearOpen}
        title={t('wl.endpoint.clear.title')}
        body={t('wl.endpoint.clear.body', { count: endpoint.capture_count })}
        confirmLabel={t('wl.endpoint.clear.confirm')}
        cancelLabel={t('wl.endpoint.clear.cancel')}
        busy={clear.isPending}
        onConfirm={clearAll}
        onOpenChange={setClearOpen}
      />
      {clearFailed ? (
        <p role="alert" className={styles.notice}>
          {t('wl.endpoint.clear.failed')}
        </p>
      ) : null}
      <Announcer announcement={live.announcement} muted={!livePrefs.announce} />
      {workspaceOpen ? null : <Outlet />}
    </section>
  );
}
