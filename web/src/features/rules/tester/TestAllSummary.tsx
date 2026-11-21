import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import styles from './Tester.module.css';
import type { TraceGroup, TraceModel } from './types';

export interface TestAllSummaryProps {
  trace: TraceModel;
  onOpen: (group: TraceGroup) => void;
}

function groupState(group: TraceGroup, t: TFunction): string {
  if (group.matched === null) {
    return t('wl.tester.all.notRun');
  }
  if (group.failedPosition !== null) {
    return t('wl.tester.all.failedAt', { position: group.failedPosition });
  }
  return group.matched
    ? t('wl.tester.all.matched', { ran: group.ranCount, total: group.stepCount })
    : t('wl.tester.status.notMatched');
}

export function TestAllSummary({ trace, onOpen }: TestAllSummaryProps) {
  const { t } = useTranslation();
  return (
    <section className={styles.section} aria-label={t('wl.tester.all.title')}>
      <h3 className={styles.sectionTitle}>{t('wl.tester.all.title')}</h3>
      <ol className={styles.rows} aria-label={t('wl.tester.all.label')}>
        {trace.groups.map((group) => (
          <li key={group.index}>
            <div className={styles.ruleRow}>
              <span className={styles.entryTitle}>
                {group.index + 1} {group.ruleName}
              </span>
              <span className={styles.entryStatus}>{groupState(group, t)}</span>
              {group.itemIds.length === 0 ? null : (
                <button
                  type="button"
                  className={`${styles.button} ${styles.small}`}
                  aria-label={t('wl.tester.all.open', { name: group.ruleName })}
                  onClick={() => {
                    onOpen(group);
                  }}
                >
                  {t('wl.tester.all.openShort')}
                </button>
              )}
            </div>
            {group.cut === 'stop' ? <p className={styles.cut}>{t('wl.tester.all.stop')}</p> : null}
            {group.cut === 'failed' ? <p className={styles.cut}>{t('wl.tester.all.failedCut')}</p> : null}
          </li>
        ))}
      </ol>
    </section>
  );
}
