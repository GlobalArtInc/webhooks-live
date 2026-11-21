import { useTranslation } from 'react-i18next';
import type { RuleRun } from '../../../api/ruleRunTypes';
import { responseFacts } from './model';
import styles from './RunLogs.module.css';

export interface ResponseSentSectionProps {
  status: number;
  runs: readonly RuleRun[];
}

export function ResponseSentSection({ status, runs }: ResponseSentSectionProps) {
  const { t } = useTranslation();
  const facts = responseFacts(status, runs);
  return (
    <dl className={styles.facts}>
      <div className={styles.fact}>
        <dt className={styles.muted}>{t('wl.runlogs.response.status')}</dt>
        <dd className={styles.mono}>{facts.status}</dd>
      </div>
      {facts.ruleError === null ? null : (
        <div className={styles.fact}>
          <dt className={styles.muted}>{t('wl.runlogs.response.ruleError')}</dt>
          <dd className={styles.error}>{facts.ruleError}</dd>
        </div>
      )}
      {facts.changedBy === null ? null : (
        <div className={styles.fact}>
          <dt className={styles.muted}>{t('wl.runlogs.response.changedBy')}</dt>
          <dd>{facts.changedBy}</dd>
        </div>
      )}
    </dl>
  );
}
