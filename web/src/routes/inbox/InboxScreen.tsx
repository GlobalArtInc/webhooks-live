import { Link, useMatch, useNavigate, useSearch } from '@tanstack/react-router';
import { lazy, Suspense, useId, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useInstance } from '../../api/queries';
import type { CaptureSummary, Endpoint } from '../../api/types';
import { CaptureList, type ListNavigation, type RowExtras } from '../../components/CaptureList/CaptureList';
import { ConnectionIndicator } from '../../components/ConnectionIndicator/ConnectionIndicator';
import { CopyButton } from '../../components/CopyButton/CopyButton';
import { useViewportLayout } from '../../features/endpoint/useViewport';
import { shortEndpointId } from '../../features/endpoints/format';
import { useMyEndpoints, type MyEndpoint } from '../../features/endpoints';
import {
  canDeleteWith,
  EndpointChip,
  formatInboxScope,
  InboxScopeSelect,
  parseInboxScope,
  queryOf,
  resolveScope,
  useDeleteInboxCapture,
  useFolders,
  useInboxCaptures,
  useInboxEndpoints,
  useInboxStream,
  type InboxScope,
} from '../../features/inbox';
import { useVisibleMinuteNow } from '../../features/list';
import styles from './InboxScreen.module.css';

const INBOX_ROUTE_ID = '/shell/inbox';
const INBOX_CAPTURE_ROUTE_ID = '/shell/inbox/$endpointId/$captureId';
const GONE = 410;
const NO_IDS: ReadonlySet<string> = new Set();

const CapturePane = lazy(() =>
  import('../../features/endpoint/CapturePane').then((module) => ({ default: module.CapturePane })),
);

function PaneLoading() {
  const { t } = useTranslation();
  return (
    <div role="status" className={styles.paneLoading}>
      {t('wl.detail.state.loading')}
    </div>
  );
}

function labelFor(endpointId: string, endpoints: ReadonlyMap<string, Endpoint>, local: readonly MyEndpoint[]): string {
  const title = endpoints.get(endpointId)?.title;
  if (title !== null && title !== undefined && title !== '') {
    return title;
  }
  const localLabel = local.find((entry) => entry.id === endpointId)?.label;
  return localLabel ?? shortEndpointId(endpointId);
}

function scopeEndpointIds(scope: InboxScope, folderEndpointIds: readonly string[]): string[] {
  switch (scope.kind) {
    case 'all':
      return [];
    case 'folder':
      return [...folderEndpointIds];
    case 'endpoints':
      return scope.endpointIds;
  }
}

export function InboxScreen() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const search = useSearch({ from: INBOX_ROUTE_ID });
  const instance = useInstance();
  const signedIn = Boolean(instance.data?.user);
  const local = useMyEndpoints();
  const folders = useFolders(signedIn);
  const layout = useViewportLayout();
  const narrow = layout === 'narrow';
  const now = useVisibleMinuteNow();
  const selectRef = useRef<HTMLSelectElement>(null);
  const [deleteFailed, setDeleteFailed] = useState(false);
  const emptyTitleId = useId();

  const chosen = useMemo(() => parseInboxScope(search.scope), [search.scope]);
  const scope = useMemo(() => resolveScope(chosen, signedIn, local), [chosen, signedIn, local]);
  const scopeKey = formatInboxScope(scope);
  const query = queryOf(scope);
  const list = useInboxCaptures(scopeKey, query);
  const items = useMemo(() => list.data?.pages.flatMap((page) => page.items) ?? [], [list.data]);
  const stream = useInboxStream(query === null ? null : scopeKey);
  const deletion = useDeleteInboxCapture(scopeKey);

  const folderEndpointIds = useMemo(() => {
    const folder = scope.kind === 'folder' ? folders.find((entry) => entry.id === scope.folderId) : undefined;
    return folder?.endpoint_ids ?? [];
  }, [scope, folders]);
  const endpointIds = useMemo(
    () => [...new Set([...scopeEndpointIds(scope, folderEndpointIds), ...items.map((item) => item.endpoint_id)])],
    [scope, folderEndpointIds, items],
  );
  const endpoints = useInboxEndpoints(endpointIds);

  const match = useMatch({ from: INBOX_CAPTURE_ROUTE_ID, shouldThrow: false });
  const selectedEndpointId = match?.params.endpointId ?? null;
  const selectedCaptureId = match?.params.captureId ?? null;

  const listGone = list.error instanceof ApiError && list.error.status === GONE;
  const streamGone = stream.endReason === 'gone' || stream.endReason === 'missing';
  const gone = listGone || streamGone;
  const empty = query === null || (list.isSuccess && items.length === 0);

  function openCapture(capture: CaptureSummary): void {
    void navigate({
      to: '/inbox/$endpointId/$captureId',
      params: { endpointId: capture.endpoint_id, captureId: capture.id },
      search: true,
    });
  }

  function closeCapture(): void {
    void navigate({ to: '/inbox', search: true });
  }

  function changeScope(next: InboxScope): void {
    setDeleteFailed(false);
    void navigate({ to: '/inbox', search: { scope: formatInboxScope(next) } });
  }

  function selectById(captureId: string): void {
    const capture = items.find((item) => item.id === captureId);
    if (capture !== undefined) {
      openCapture(capture);
    }
  }

  function navigateList(navigation: ListNavigation): void {
    const index = items.findIndex((item) => item.id === selectedCaptureId);
    let target: CaptureSummary | undefined;
    switch (navigation) {
      case 'first':
        target = items[0];
        break;
      case 'last':
        target = items.at(-1);
        break;
      case 'previous':
        target = index > 0 ? items[index - 1] : undefined;
        break;
      case 'next':
        target = index === -1 ? items[0] : items[index + 1];
        break;
    }
    if (target !== undefined) {
      openCapture(target);
    }
  }

  function requestDelete(captureId: string): void {
    const capture = items.find((item) => item.id === captureId);
    if (capture === undefined) {
      return;
    }
    setDeleteFailed(false);
    deletion.mutate(
      { endpointId: capture.endpoint_id, captureId },
      {
        onSuccess: () => {
          if (selectedCaptureId === captureId) {
            closeCapture();
          }
        },
        onError: () => {
          setDeleteFailed(true);
        },
      },
    );
  }

  function rowExtrasOf(item: CaptureSummary): RowExtras {
    const endpoint = endpoints.get(item.endpoint_id);
    return {
      endpoint: <EndpointChip endpointId={item.endpoint_id} label={labelFor(item.endpoint_id, endpoints, local)} />,
      deleteDenied: canDeleteWith(endpoint) ? undefined : t('wl.inbox.row.deleteDenied'),
    };
  }

  const showDetail = selectedCaptureId !== null && selectedEndpointId !== null;
  const hideList = narrow && showDetail;
  const addresses = scopeEndpointIds(scope, folderEndpointIds)
    .map((id) => endpoints.get(id))
    .filter((endpoint): endpoint is Endpoint => endpoint !== undefined);

  return (
    <section className={styles.screen}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t('wl.inbox.screen')}</h1>
        <InboxScopeSelect
          scope={chosen}
          signedIn={signedIn}
          folders={folders}
          local={local}
          selectRef={selectRef}
          onChange={changeScope}
        />
        <ConnectionIndicator state={stream.connection} lastEventAt={stream.lastEventAt} now={now} />
      </header>
      {gone ? (
        <div role="alert" className={styles.gone}>
          <p className={styles.goneTitle}>{t('wl.inbox.gone.title')}</p>
          <p className={styles.hint}>{t('wl.inbox.gone.body')}</p>
          <button
            type="button"
            className={styles.button}
            onClick={() => {
              selectRef.current?.focus();
            }}
          >
            {t('wl.inbox.gone.choose')}
          </button>
        </div>
      ) : null}
      {deleteFailed ? (
        <p role="alert" className={styles.notice}>
          {t('wl.inbox.delete.failed')}
        </p>
      ) : null}
      <div className={styles.body}>
        {hideList ? null : (
          <div className={styles.list}>
            {empty ? (
              <section className={styles.empty} aria-labelledby={emptyTitleId}>
                <h2 id={emptyTitleId} className={styles.emptyTitle}>
                  {query === null ? t('wl.inbox.empty.none') : t('wl.inbox.empty.title')}
                </h2>
                {query === null ? null : <p className={styles.hint}>{t('wl.inbox.empty.body')}</p>}
                {addresses.length === 0 ? null : (
                  <ul className={styles.addresses}>
                    {addresses.map((endpoint) => (
                      <li key={endpoint.id} className={styles.address}>
                        <code className={styles.url}>{endpoint.url}</code>
                        <CopyButton value={endpoint.url} label={t('wl.inbox.empty.copy')} />
                      </li>
                    ))}
                  </ul>
                )}
              </section>
            ) : (
              <CaptureList
                items={items}
                selectedId={selectedCaptureId}
                markedId={null}
                variant={narrow ? 'two-line' : 'one-line'}
                now={now}
                unreadIds={NO_IDS}
                arrivingIds={NO_IDS}
                following={false}
                total={null}
                pending={query !== null && list.isPending}
                failed={list.isError && items.length === 0 && !gone}
                pageFailed={list.isError && items.length > 0 && !gone}
                hasNextPage={list.hasNextPage}
                isFetchingNextPage={list.isFetchingNextPage}
                onNavigate={navigateList}
                onSelect={selectById}
                onDelete={requestDelete}
                onCompare={() => undefined}
                onLoadMore={() => {
                  void list.fetchNextPage();
                }}
                onRetry={() => {
                  void list.refetch();
                }}
                onAtTopChange={() => undefined}
                onPillActivate={() => undefined}
                rowExtras={rowExtrasOf}
                compare={false}
              />
            )}
          </div>
        )}
        {showDetail && selectedEndpointId !== null && selectedCaptureId !== null ? (
          <div className={hideList ? styles.detailNarrow : styles.detail}>
            <header className={styles.detailHeader}>
              <EndpointChip endpointId={selectedEndpointId} label={labelFor(selectedEndpointId, endpoints, local)} />
              <Link to="/e/$id" params={{ id: selectedEndpointId }} className={styles.link}>
                {t('wl.inbox.detail.open')}
              </Link>
            </header>
            <Suspense fallback={<PaneLoading />}>
              <CapturePane
                endpointId={selectedEndpointId}
                captureId={selectedCaptureId}
                markedId={null}
                narrow={narrow}
                pager={null}
                onBack={narrow ? closeCapture : undefined}
              />
            </Suspense>
          </div>
        ) : narrow ? null : (
          <div className={styles.placeholder}>{t('wl.inbox.detail.none')}</div>
        )}
      </div>
    </section>
  );
}
