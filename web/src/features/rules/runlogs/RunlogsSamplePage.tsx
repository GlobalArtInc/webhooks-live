import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CaptureRulesView } from './CaptureRulesSection';
import { captureRunsFixture } from './fixtures';
import { ResponseSentSection } from './ResponseSentSection';
import { RuleRunsView } from './RuleRunsPanel';
import styles from './RunlogsSamplePage.module.css';

const SAMPLE_STATUS = 200;
const NOOP = (): void => undefined;

export function RunlogsSamplePage() {
  const { t } = useTranslation();
  const [failedOnly, setFailedOnly] = useState(false);
  const runs = failedOnly ? captureRunsFixture.filter((run) => run.state === 'failed') : captureRunsFixture;
  return (
    <main className={styles.page}>
      <h1 className={styles.title}>{t('wl.runlogs.section.response')}</h1>
      <section className={styles.section} aria-label={t('wl.runlogs.section.response')}>
        <ResponseSentSection status={SAMPLE_STATUS} runs={captureRunsFixture} />
      </section>
      <section className={styles.section} aria-label={t('wl.runlogs.section.rules')}>
        <h2 className={styles.heading}>{t('wl.runlogs.section.rules')}</h2>
        <CaptureRulesView
          runs={captureRunsFixture}
          canRerun
          busy={false}
          queued={false}
          failure={null}
          onRerun={NOOP}
        />
      </section>
      <section className={styles.section}>
        <h2 className={styles.heading}>{t('wl.runlogs.runs.label')}</h2>
        <RuleRunsView
          runs={runs}
          status="ready"
          failedOnly={failedOnly}
          onFailedOnlyChange={setFailedOnly}
          hasMore={false}
          loadingMore={false}
          onLoadMore={NOOP}
        />
      </section>
    </main>
  );
}
