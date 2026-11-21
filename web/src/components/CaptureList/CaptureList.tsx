import { useVirtualizer } from '@tanstack/react-virtual';
import { useEffect, useId, useRef, useState, type KeyboardEvent, type UIEvent } from 'react';
import { useTranslation } from 'react-i18next';
import type { CaptureSummary } from '../../api/types';
import { compareLabelOf } from '../../features/list/compare';
import { countAbove, isAtTop } from '../../features/live/scrollAnchor';
import { NewPill } from '../NewPill/NewPill';
import { CaptureRow, CaptureRowActions, type CaptureRowVariant } from '../CaptureRow/CaptureRow';
import styles from './CaptureList.module.css';

export type ListNavigation = 'first' | 'previous' | 'next' | 'last';

export const ROW_HEIGHT_ONE_LINE = 36;
export const ROW_HEIGHT_TWO_LINE = 56;
export const LOAD_AHEAD_ROWS = 10;
export const OVERSCAN_ROWS = 10;

const SKELETON_ROWS = 6;

const KEY_NAVIGATION: Readonly<Record<string, ListNavigation>> = {
  ArrowDown: 'next',
  ArrowUp: 'previous',
  Home: 'first',
  End: 'last',
};

export interface CaptureListSelection {
  active: boolean;
  checkedIds: ReadonlySet<string>;
  onToggle: (captureId: string) => void;
  onLongPress?: (captureId: string) => void;
}

export interface CaptureListProps {
  items: CaptureSummary[];
  selectedId: string | null;
  markedId: string | null;
  variant: CaptureRowVariant;
  now: number;
  unreadIds: ReadonlySet<string>;
  arrivingIds: ReadonlySet<string>;
  following: boolean;
  total: number | null;
  pending: boolean;
  failed: boolean;
  pageFailed: boolean;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  onNavigate: (navigation: ListNavigation) => void;
  onSelect: (captureId: string) => void;
  onDelete: (captureId: string) => void;
  onCompare: (captureId: string) => void;
  onLoadMore: () => void;
  onRetry: () => void;
  onAtTopChange: (atTop: boolean) => void;
  onPillActivate: () => void;
  selection?: CaptureListSelection;
}

export function CaptureList({
  items,
  selectedId,
  markedId,
  variant,
  now,
  unreadIds,
  arrivingIds,
  following,
  total,
  pending,
  failed,
  pageFailed,
  hasNextPage,
  isFetchingNextPage,
  onNavigate,
  onSelect,
  onDelete,
  onCompare,
  onLoadMore,
  onRetry,
  onAtTopChange,
  onPillActivate,
  selection,
}: CaptureListProps) {
  const { t } = useTranslation();
  const idPrefix = useId();
  const scrollRef = useRef<HTMLDivElement>(null);
  const scrolledFor = useRef<string | null>(null);
  const [awayFrom, setAwayFrom] = useState<string | null>(null);
  const rowHeight = variant === 'two-line' ? ROW_HEIGHT_TWO_LINE : ROW_HEIGHT_ONE_LINE;
  const selectedIndex = selectedId === null ? -1 : items.findIndex((item) => item.id === selectedId);
  const setSize = total ?? items.length;

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: OVERSCAN_ROWS,
    anchorTo: awayFrom === null ? 'start' : 'end',
    getItemKey: (index) => items[index]?.id ?? index,
  });

  const virtualRows = virtualizer.getVirtualItems();
  const lastVisible = virtualRows.at(-1)?.index ?? -1;

  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage && !pageFailed && lastVisible >= items.length - LOAD_AHEAD_ROWS) {
      onLoadMore();
    }
  }, [lastVisible, items.length, hasNextPage, isFetchingNextPage, pageFailed, onLoadMore]);

  useEffect(() => {
    if (selectedId === null || selectedIndex === -1 || scrolledFor.current === selectedId) {
      return;
    }
    scrolledFor.current = selectedId;
    if (following && selectedIndex === 0) {
      return;
    }
    virtualizer.scrollToIndex(selectedIndex, { align: 'auto' });
  }, [selectedId, selectedIndex, following, virtualizer]);

  const atTop = awayFrom === null;
  useEffect(() => {
    onAtTopChange(atTop);
  }, [atTop, onAtTopChange]);

  const arrivedCount = countAbove(items, awayFrom);

  function handleScroll(event: UIEvent<HTMLDivElement>) {
    const top = isAtTop(event.currentTarget.scrollTop);
    setAwayFrom((current) => (top ? null : (current ?? items[0]?.id ?? null)));
  }

  function activatePill() {
    const scroller = scrollRef.current;
    if (scroller !== null) {
      scroller.scrollTop = 0;
    }
    setAwayFrom(null);
    scroller?.querySelector<HTMLElement>('[role="listbox"]')?.focus({ preventScroll: true });
    onPillActivate();
  }

  const renderedIndexes = virtualRows.map((row) => row.index);
  if (selectedIndex !== -1 && !renderedIndexes.includes(selectedIndex)) {
    renderedIndexes.push(selectedIndex);
  }
  const rows = renderedIndexes.flatMap((index) => {
    const item = items[index];
    return item === undefined ? [] : [{ index, item }];
  });
  const height = virtualizer.getTotalSize();

  function optionId(captureId: string): string {
    return `${idPrefix}-${captureId}`;
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === ' ' && selection !== undefined && selectedId !== null) {
      event.preventDefault();
      selection.onToggle(selectedId);
      return;
    }
    const navigation = KEY_NAVIGATION[event.key];
    if (navigation === undefined) {
      return;
    }
    event.preventDefault();
    onNavigate(navigation);
  }

  const selectedOptionId = selectedId !== null && selectedIndex !== -1 ? optionId(selectedId) : undefined;

  return (
    <div className={styles.root}>
      <NewPill count={arrivedCount} onActivate={activatePill} />
      <div ref={scrollRef} className={styles.scroller} onScroll={handleScroll}>
        {pending ? (
          <div className={styles.skeleton} role="status">
            <span className={styles.visuallyHidden}>{t('wl.endpoint.list.loading')}</span>
            {Array.from({ length: SKELETON_ROWS }, (_value, index) => (
              <span key={index} className={styles.skeletonRow} />
            ))}
          </div>
        ) : (
          <>
            <div
              role="listbox"
              tabIndex={0}
              aria-label={t('wl.endpoint.list.label')}
              aria-activedescendant={selectedOptionId}
              aria-multiselectable={selection?.active ? true : undefined}
              className={styles.listbox}
              style={{ height }}
              onKeyDown={handleKeyDown}
            >
              {rows.map(({ index, item }) => (
                <CaptureRow
                  key={item.id}
                  optionId={optionId(item.id)}
                  summary={item}
                  variant={variant}
                  selected={item.id === selectedId}
                  positionInSet={index + 1}
                  setSize={setSize}
                  now={now}
                  unread={unreadIds.has(item.id)}
                  arriving={arrivingIds.has(item.id)}
                  checkable={selection?.active ?? false}
                  checked={selection?.checkedIds.has(item.id) ?? false}
                  style={{ height: rowHeight, transform: `translateY(${index * rowHeight}px)` }}
                  onSelect={selection?.active === true ? selection.onToggle : onSelect}
                  onLongPress={selection?.onLongPress}
                />
              ))}
            </div>
            <div className={styles.actionLayer} style={{ height }}>
              {rows.map(({ index, item }) => (
                <CaptureRowActions
                  key={item.id}
                  captureId={item.id}
                  compareLabel={compareLabelOf(markedId, item.id)}
                  style={{ height: rowHeight, transform: `translateY(${index * rowHeight}px)` }}
                  onDelete={onDelete}
                  onCompare={onCompare}
                />
              ))}
            </div>
          </>
        )}
      </div>
      {failed ? (
        <div className={styles.error} role="alert">
          <span>{t('wl.endpoint.list.error.title')}</span>
          <button type="button" className={styles.button} onClick={onRetry}>
            {t('wl.endpoint.list.error.retry')}
          </button>
        </div>
      ) : null}
      {pageFailed ? (
        <div className={styles.error} role="alert">
          <span>{t('wl.endpoint.list.pageError')}</span>
          <button type="button" className={styles.button} onClick={onRetry}>
            {t('wl.endpoint.list.error.retry')}
          </button>
        </div>
      ) : null}
      {hasNextPage && !pending ? (
        <div className={styles.more}>
          <button type="button" className={styles.button} onClick={onLoadMore} disabled={isFetchingNextPage}>
            {isFetchingNextPage ? t('wl.endpoint.list.loadingMore') : t('wl.endpoint.list.loadMore')}
          </button>
        </div>
      ) : null}
    </div>
  );
}
