import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../../api/client';
import type { RuleRun } from '../../../api/ruleRunTypes';
import { ConfirmDialog } from '../../../components/ConfirmDialog/ConfirmDialog';
import { useRerunCapture } from './api';
import { RuleRunList } from './RuleRunList';
import styles from './RunLogs.module.css';

const CONFLICT = 409;
const UNPROCESSABLE = 422;
const LIMIT_CODE = 'limit_reached';

export type RerunFailureKind = 'active' | 'limit' | 'failed';

export interface RerunFailure {
  kind: RerunFailureKind;
  max: number;
}

export interface CaptureRulesViewProps {
  runs: readonly RuleRun[];
  canRerun: boolean;
  busy: boolean;
  queued: boolean;
  failure: RerunFailure | null;
  onRerun: () => void;
}

export interface CaptureRulesSectionProps {
  endpointId: string;
  captureId: string;
  runs: readonly RuleRun[];
  canRerun: boolean;
}

export function rerunFailureOf(error: Error | null): RerunFailure | null {
  if (error === null) {
    return null;
  }
  if (!(error instanceof ApiError)) {
    return { kind: 'failed', max: 0 };
  }
  if (error.status === CONFLICT) {
    return { kind: 'active', max: 0 };
  }
  if (error.status === UNPROCESSABLE && error.code === LIMIT_CODE) {
    return { kind: 'limit', max: error.limit?.max ?? 0 };
  }
  return { kind: 'failed', max: 0 };
}

export function CaptureRulesView({ runs, canRerun, busy, queued, failure, onRerun }: CaptureRulesViewProps) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const failureText = (): string => {
    if (failure === null) {
      return '';
    }
    if (failure.kind === 'active') {
      return t('wl.runlogs.rerun.active');
    }
    return failure.kind === 'limit'
      ? t('wl.runlogs.rerun.limit', { count: failure.max })
      : t('wl.runlogs.rerun.failed');
  };
  return (
    <div className={styles.panel}>
      {canRerun ? (
        <div className={styles.toolbar}>
          <button
            type="button"
            className={styles.action}
            disabled={busy}
            onClick={() => {
              setConfirming(true);
            }}
          >
            {t('wl.runlogs.rerun.button')}
          </button>
        </div>
      ) : null}
      {queued && !busy ? (
        <p role="status" className={styles.muted}>
          {t('wl.runlogs.rerun.queued')}
        </p>
      ) : null}
      {failure === null ? null : (
        <p role="alert" className={styles.error}>
          {failureText()}
        </p>
      )}
      {runs.length === 0 ? (
        <p className={styles.muted}>{t('wl.runlogs.run.none')}</p>
      ) : (
        <RuleRunList runs={runs} label={t('wl.runlogs.section.rules')} />
      )}
      <ConfirmDialog
        open={confirming}
        title={t('wl.runlogs.rerun.title')}
        body={t('wl.runlogs.rerun.body')}
        confirmLabel={t('wl.runlogs.rerun.confirm')}
        cancelLabel={t('wl.runlogs.rerun.cancel')}
        busy={busy}
        onConfirm={() => {
          setConfirming(false);
          onRerun();
        }}
        onOpenChange={setConfirming}
      />
    </div>
  );
}

export function CaptureRulesSection({ endpointId, captureId, runs, canRerun }: CaptureRulesSectionProps) {
  const rerun = useRerunCapture(endpointId, captureId);
  return (
    <CaptureRulesView
      runs={runs}
      canRerun={canRerun}
      busy={rerun.isPending}
      queued={rerun.isSuccess}
      failure={rerunFailureOf(rerun.error)}
      onRerun={() => {
        rerun.mutate();
      }}
    />
  );
}
