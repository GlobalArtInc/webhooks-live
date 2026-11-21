import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RuleRun, RuleRunFilter } from '../../../api/ruleRunTypes';
import { useRuleRuns } from './api';
import { RuleRunList } from './RuleRunList';
import styles from './RunLogs.module.css';

export type RuleRunsStatus = 'loading' | 'error' | 'ready';

export interface RuleRunsViewProps {
  runs: readonly RuleRun[];
  status: RuleRunsStatus;
  failedOnly: boolean;
  onFailedOnlyChange: (failedOnly: boolean) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
}

export interface RuleRunsPanelProps {
  endpointId: string;
}

const FAILED_FILTER: RuleRunFilter = { state: 'failed' };
const NO_FILTER: RuleRunFilter = {};

export function RuleRunsView({
  runs,
  status,
  failedOnly,
  onFailedOnlyChange,
  hasMore,
  loadingMore,
  onLoadMore,
}: RuleRunsViewProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.panel}>
      <div className={styles.toolbar}>
        <label className={styles.filter}>
          <input
            type="checkbox"
            checked={failedOnly}
            onChange={(event) => {
              onFailedOnlyChange(event.currentTarget.checked);
            }}
          />
          {t('wl.runlogs.runs.failedOnly')}
        </label>
      </div>
      {status === 'loading' ? (
        <p role="status" className={styles.muted}>
          {t('wl.runlogs.runs.loading')}
        </p>
      ) : null}
      {status === 'error' ? (
        <p role="alert" className={styles.error}>
          {t('wl.runlogs.runs.error')}
        </p>
      ) : null}
      {status === 'ready' && runs.length === 0 ? <p className={styles.muted}>{t('wl.runlogs.runs.empty')}</p> : null}
      {runs.length === 0 ? null : <RuleRunList runs={runs} label={t('wl.runlogs.runs.label')} />}
      {hasMore ? (
        <div className={styles.toolbar}>
          <button type="button" className={styles.action} disabled={loadingMore} onClick={onLoadMore}>
            {t('wl.runlogs.runs.loadMore')}
          </button>
        </div>
      ) : null}
    </div>
  );
}

export function RuleRunsPanel({ endpointId }: RuleRunsPanelProps) {
  const [failedOnly, setFailedOnly] = useState(false);
  const filter = failedOnly ? FAILED_FILTER : NO_FILTER;
  const query = useRuleRuns(endpointId, filter);
  const runs = query.data?.pages.flatMap((page) => page.items) ?? [];
  const status: RuleRunsStatus = query.isError ? 'error' : query.isPending ? 'loading' : 'ready';
  return (
    <RuleRunsView
      runs={runs}
      status={status}
      failedOnly={failedOnly}
      onFailedOnlyChange={setFailedOnly}
      hasMore={query.hasNextPage}
      loadingMore={query.isFetchingNextPage}
      onLoadMore={() => {
        void query.fetchNextPage();
      }}
    />
  );
}
