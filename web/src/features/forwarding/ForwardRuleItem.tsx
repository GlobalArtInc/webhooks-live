import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import {
  useDeleteForwardRule,
  usePatchForwardRule,
  useTestForwardRule,
  type RuleTestResult,
} from '../../api/forwardRules';
import { useCapture } from '../../api/queries';
import type { ForwardJob, ForwardRule } from '../../api/types';
import { ForwardResultPanel } from '../../components/ForwardResultPanel/ForwardResultPanel';
import { ForwardRuleEditor } from './ForwardRuleEditor';
import styles from './ForwardRuleItem.module.css';

const NO_CAPTURE_CODE = 'capture_not_found';

export interface ForwardRuleItemProps {
  endpointId: string;
  rule: ForwardRule;
  readOnly: boolean;
}

interface RuleTestOutcomeProps {
  endpointId: string;
  run: RuleTestResult;
  onRetry: () => void;
}

function RuleTestOutcome({ endpointId, run, onRetry }: RuleTestOutcomeProps) {
  const detail = useCapture(endpointId, run.captureId);
  const latest: ForwardJob = detail.data?.forwards.find((job) => job.id === run.job.id) ?? run.job;
  return <ForwardResultPanel results={[latest]} onResend={onRetry} />;
}

export function ForwardRuleItem({ endpointId, rule, readOnly }: ForwardRuleItemProps) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [run, setRun] = useState<RuleTestResult | null>(null);
  const [noCapture, setNoCapture] = useState(false);
  const [testFailed, setTestFailed] = useState(false);
  const [toggleFailed, setToggleFailed] = useState(false);
  const [deleteFailed, setDeleteFailed] = useState(false);
  const patch = usePatchForwardRule(endpointId);
  const remove = useDeleteForwardRule(endpointId);
  const test = useTestForwardRule(endpointId);

  function runTest(): void {
    setNoCapture(false);
    setTestFailed(false);
    test.mutate(rule.id, {
      onSuccess: (result) => {
        setRun(result);
      },
      onError: (failure) => {
        const missing = failure instanceof ApiError && failure.code === NO_CAPTURE_CODE;
        setNoCapture(missing);
        setTestFailed(!missing);
      },
    });
  }

  function toggleEnabled(): void {
    setToggleFailed(false);
    patch.mutate(
      { ruleId: rule.id, input: { enabled: !rule.enabled } },
      {
        onError: () => {
          setToggleFailed(true);
        },
      },
    );
  }

  function deleteRule(): void {
    setDeleteFailed(false);
    remove.mutate(rule.id, {
      onError: () => {
        setDeleteFailed(true);
      },
    });
  }

  return (
    <article className={styles.item}>
      <div className={styles.summary}>
        <code className={styles.target}>{rule.target_url}</code>
        <button
          type="button"
          role="switch"
          aria-checked={rule.enabled}
          aria-label={t('wl.forwarding.rule.enabledFor', { target: rule.target_url })}
          className={rule.enabled ? `${styles.switch} ${styles.on}` : styles.switch}
          disabled={readOnly || patch.isPending}
          onClick={toggleEnabled}
        >
          <span className={styles.knob} aria-hidden="true" />
          <span>{rule.enabled ? t('wl.forwarding.rule.on') : t('wl.forwarding.rule.off')}</span>
        </button>
      </div>
      <div className={styles.actions}>
        <button
          type="button"
          className={styles.button}
          aria-expanded={editing}
          disabled={readOnly}
          onClick={() => {
            setEditing((current) => !current);
          }}
        >
          {t('wl.forwarding.rule.edit')}
        </button>
        <button type="button" className={styles.button} disabled={readOnly || test.isPending} onClick={runTest}>
          {t('wl.forwarding.rule.test')}
        </button>
        <button type="button" className={styles.danger} disabled={readOnly || remove.isPending} onClick={deleteRule}>
          {t('wl.forwarding.rule.delete')}
        </button>
      </div>
      {toggleFailed ? (
        <p role="alert" className={styles.error}>
          {t('wl.forwarding.error.toggle')}
        </p>
      ) : null}
      {deleteFailed ? (
        <p role="alert" className={styles.error}>
          {t('wl.forwarding.error.delete')}
        </p>
      ) : null}
      {noCapture ? <p className={styles.note}>{t('wl.forwarding.rule.noCapture')}</p> : null}
      {testFailed ? (
        <p role="alert" className={styles.error}>
          {t('wl.forwarding.error.test')}
        </p>
      ) : null}
      {run === null ? null : <RuleTestOutcome endpointId={endpointId} run={run} onRetry={runTest} />}
      {editing ? (
        <ForwardRuleEditor
          endpointId={endpointId}
          rule={rule}
          onDone={() => {
            setEditing(false);
          }}
          onCancel={() => {
            setEditing(false);
          }}
        />
      ) : null}
    </article>
  );
}
