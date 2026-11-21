import { useTranslation } from 'react-i18next';
import type { RuleRunState, RuleStepOutcome } from '../../../api/ruleRunTypes';
import styles from './RunLogs.module.css';

export type BadgeTone = 'ok' | 'failed' | 'neutral';

export interface RunStateBadgeProps {
  state: RuleRunState;
}

export interface StepOutcomeBadgeProps {
  outcome: RuleStepOutcome;
}

export function RunStateBadge({ state }: RunStateBadgeProps) {
  const { t } = useTranslation();
  const labels: Record<RuleRunState, string> = {
    sync: t('wl.runlogs.state.sync'),
    queued: t('wl.runlogs.state.queued'),
    running: t('wl.runlogs.state.running'),
    succeeded: t('wl.runlogs.state.succeeded'),
    failed: t('wl.runlogs.state.failed'),
  };
  const tones: Record<RuleRunState, BadgeTone> = {
    sync: 'neutral',
    queued: 'neutral',
    running: 'neutral',
    succeeded: 'ok',
    failed: 'failed',
  };
  return (
    <span className={styles.badge} data-tone={tones[state]}>
      {labels[state]}
    </span>
  );
}

export function StepOutcomeBadge({ outcome }: StepOutcomeBadgeProps) {
  const { t } = useTranslation();
  const labels: Record<RuleStepOutcome, string> = {
    ok: t('wl.runlogs.outcome.ok'),
    failed: t('wl.runlogs.outcome.failed'),
    skipped: t('wl.runlogs.outcome.skipped'),
    deferred: t('wl.runlogs.outcome.deferred'),
    no_op: t('wl.runlogs.outcome.noOp'),
  };
  const tones: Record<RuleStepOutcome, BadgeTone> = {
    ok: 'ok',
    failed: 'failed',
    skipped: 'neutral',
    deferred: 'neutral',
    no_op: 'neutral',
  };
  return (
    <span className={styles.badge} data-tone={tones[outcome]}>
      {labels[outcome]}
    </span>
  );
}
