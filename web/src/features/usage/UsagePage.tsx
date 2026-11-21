import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useUsage } from '../../api/usage';
import { StatePage } from '../../components/StatePage/StatePage';
import { stateKindForError } from '../../components/StatePage/stateKind';
import { useNow } from '../endpoints/useNow';
import { UsageMeterItem } from './UsageMeterItem';
import { UsageUnavailable } from './UsageUnavailable';
import styles from './UsagePage.module.css';

function isUnavailableError(error: unknown): boolean {
  if (!(error instanceof ApiError)) {
    return false;
  }
  return (
    (error.status === 401 && error.code === 'unauthenticated') ||
    (error.status === 403 && error.code === 'insufficient_scope')
  );
}

export function UsagePage() {
  const { t, i18n } = useTranslation();
  const usage = useUsage(true);
  const now = useNow();

  if (usage.isError && usage.data === undefined) {
    if (isUnavailableError(usage.error)) {
      return <UsageUnavailable />;
    }
    return (
      <StatePage
        kind={stateKindForError(usage.error)}
        detail={usage.error instanceof ApiError ? { status: usage.error.status } : {}}
        onRetry={() => void usage.refetch()}
      />
    );
  }

  const data = usage.data;
  return (
    <section className={styles.page} aria-labelledby="usage-title">
      <header className={styles.header}>
        <h1 id="usage-title" className={styles.title}>
          {t('wl.usage.title')}
        </h1>
        <p className={styles.description}>{t('wl.usage.description')}</p>
        {data !== undefined && <p className={styles.tier}>{t('wl.usage.tier', { tier: data.tier })}</p>}
      </header>

      {data === undefined && (
        <p role="status" className={styles.loading}>
          {t('wl.usage.loading')}
        </p>
      )}
      {data !== undefined && data.items.length === 0 && <p className={styles.empty}>{t('wl.usage.empty')}</p>}
      {data !== undefined && data.items.length > 0 && (
        <ul className={styles.meters}>
          {data.items.map((meter) => (
            <li key={meter.name}>
              <UsageMeterItem meter={meter} now={now} locale={i18n.language} />
            </li>
          ))}
        </ul>
      )}

      <p className={styles.footer}>{t('wl.usage.footer')}</p>
    </section>
  );
}
