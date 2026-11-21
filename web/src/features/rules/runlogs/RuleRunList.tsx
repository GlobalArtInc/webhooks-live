import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import type { RuleRun, RuleRunStepEntry, RuleRunTrigger } from '../../../api/ruleRunTypes';
import { formatAbsoluteTime } from '../../../components/format/format';
import { groupEntries, outputText, totalDuration, type RuleGroup } from './model';
import { RunStateBadge, StepOutcomeBadge } from './RunStateBadge';
import styles from './RunLogs.module.css';

export interface RuleRunListProps {
  runs: readonly RuleRun[];
  label: string;
  testerEndpointId?: string;
}

interface RuleRunCardProps {
  run: RuleRun;
  open: boolean;
  testerEndpointId: string | undefined;
}

interface RuleGroupRowProps {
  group: RuleGroup;
  run: RuleRun;
  testerEndpointId: string | undefined;
}

interface StepRowProps {
  step: RuleRunStepEntry;
}

export function RuleRunList({ runs, label, testerEndpointId }: RuleRunListProps) {
  return (
    <ol aria-label={label} className={styles.runs}>
      {runs.map((run, index) => (
        <li key={run.id}>
          <RuleRunCard run={run} open={index === 0} testerEndpointId={testerEndpointId} />
        </li>
      ))}
    </ol>
  );
}

function RuleRunCard({ run, open, testerEndpointId }: RuleRunCardProps) {
  const { t, i18n } = useTranslation();
  const triggers: Record<RuleRunTrigger, string> = {
    capture: t('wl.runlogs.trigger.capture'),
    rerun: t('wl.runlogs.trigger.rerun'),
    dry_run: t('wl.runlogs.trigger.dryRun'),
  };
  const groups = groupEntries(run.entries);
  return (
    <details className={styles.run} open={open}>
      <summary className={styles.runSummary}>
        <RunStateBadge state={run.state} />
        <span>{triggers[run.trigger]}</span>
        <time dateTime={run.created_at} className={styles.muted}>
          {formatAbsoluteTime(run.created_at, i18n.language)}
        </time>
        {run.response_status === null ? null : (
          <span className={styles.mono}>{t('wl.runlogs.run.status', { status: run.response_status })}</span>
        )}
      </summary>
      {run.error === null ? null : <p className={styles.error}>{run.error}</p>}
      {groups.length === 0 ? (
        <p className={styles.muted}>{t('wl.runlogs.run.noRules')}</p>
      ) : (
        <ul className={styles.rules}>
          {groups.map((group) => (
            <RuleGroupRow key={group.ruleId} group={group} run={run} testerEndpointId={testerEndpointId} />
          ))}
        </ul>
      )}
    </details>
  );
}

function RuleGroupRow({ group, run, testerEndpointId }: RuleGroupRowProps) {
  const { t } = useTranslation();
  const name = group.ruleName ?? t('wl.runlogs.rule.unnamed');
  const duration = totalDuration(group.steps);
  return (
    <li className={styles.rule}>
      <h3 className={styles.ruleTitle}>{name}</h3>
      {testerEndpointId === undefined ? null : (
        <Link
          to="/e/$id/settings/rules/$ruleId"
          params={{ id: testerEndpointId, ruleId: group.ruleId }}
          search={{ tab: 'test', capture: run.capture_id, run: run.id }}
          className={`${styles.action} ${styles.actionLink}`}
          aria-label={t('wl.runlogs.rule.openInTesterFor', { name })}
        >
          {t('wl.runlogs.rule.openInTester')}
        </Link>
      )}
      <p className={styles.meta}>
        <span className={group.matched ? styles.matchedOk : styles.matchedNone}>
          {group.matched ? t('wl.runlogs.rule.matched') : t('wl.runlogs.rule.notMatched')}
        </span>
        {group.steps.length === 0 ? null : (
          <span>
            {' · '}
            {t('wl.runlogs.rule.steps', { count: group.steps.length })}
            {' · '}
            {t('wl.runlogs.duration', { milliseconds: duration })}
          </span>
        )}
      </p>
      {group.steps.length === 0 ? null : (
        <ol className={styles.steps}>
          {group.steps.map((step) => (
            <StepRow key={`${step.rule_id}-${step.step}`} step={step} />
          ))}
        </ol>
      )}
    </li>
  );
}

function StepRow({ step }: StepRowProps) {
  const { t } = useTranslation();
  const output = outputText(step.output);
  return (
    <li className={styles.step}>
      <div className={styles.stepHead}>
        <span className={styles.mono}>{t('wl.runlogs.step.number', { number: step.step })}</span>
        <span className={styles.mono}>{step.name ?? step.kind}</span>
        <StepOutcomeBadge outcome={step.outcome} />
        <span className={styles.muted}>{t('wl.runlogs.duration', { milliseconds: step.duration_ms })}</span>
      </div>
      {step.detail === null ? null : <p className={styles.stepDetail}>{step.detail}</p>}
      {output === null ? null : (
        <details className={styles.output}>
          <summary>{t('wl.runlogs.step.output')}</summary>
          <pre
            className={styles.outputText}
            tabIndex={0}
            aria-label={t('wl.runlogs.step.outputOf', { step: step.step })}
          >
            {output.text}
          </pre>
          {output.truncated ? <p className={styles.muted}>{t('wl.runlogs.step.truncated')}</p> : null}
        </details>
      )}
    </li>
  );
}
