import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { SentRequest } from '../../api/types';
import { formatAbsoluteTime } from '../../components/format/format';
import { MethodBadge } from '../../components/MethodBadge/MethodBadge';
import { StatusChip } from '../../components/StatusChip/StatusChip';
import { useComposerMessages } from './messages';
import type { HistoryFailure } from './sendErrors';
import styles from './HistoryList.module.css';

export interface HistoryListProps {
  entries: readonly SentRequest[] | undefined;
  loading: boolean;
  failure: HistoryFailure | null;
  offered: boolean;
  signInAvailable: boolean;
  deleting: boolean;
  onReplay: (entry: SentRequest) => void;
  onDelete: (entry: SentRequest) => void;
}

export function HistoryList({
  entries,
  loading,
  failure,
  offered,
  signInAvailable,
  deleting,
  onReplay,
  onDelete,
}: HistoryListProps) {
  const { t, i18n } = useTranslation();
  const { historyText } = useComposerMessages();
  const headingId = useId();
  const items = entries ?? [];

  return (
    <section aria-labelledby={headingId} className={styles.section}>
      <h2 id={headingId} className={styles.heading}>
        {t('wl.composer.history.title')}
      </h2>
      {loading ? <p className={styles.note}>{t('wl.composer.history.loading')}</p> : null}
      {failure === null ? null : (
        <p role="alert" className={styles.error}>
          {historyText(failure)}
        </p>
      )}
      {loading || offered ? null : (
        <p className={styles.note}>
          {signInAvailable ? t('wl.composer.history.signIn') : t('wl.composer.history.noSignIn')}
        </p>
      )}
      {loading || !offered || failure !== null || items.length > 0 ? null : (
        <p className={styles.note}>{t('wl.composer.history.empty')}</p>
      )}
      {items.length === 0 ? null : (
        <ul className={styles.list}>
          {items.map((entry) => {
            const target = entry.request.url;
            const method = entry.request.method;
            return (
              <li key={entry.id} className={styles.item}>
                <MethodBadge method={method} />
                <span className={styles.target}>{target}</span>
                {entry.status === null ? (
                  <span className={styles.note}>{t('wl.composer.response.noStatus')}</span>
                ) : (
                  <StatusChip code={entry.status} kind="answered" label={String(entry.status)} />
                )}
                <time className={styles.note} dateTime={entry.created_at}>
                  {formatAbsoluteTime(entry.created_at, i18n.language)}
                </time>
                <div className={styles.actions}>
                  <button
                    type="button"
                    className={styles.button}
                    aria-label={t('wl.composer.history.replayItem', { method, target })}
                    onClick={() => {
                      onReplay(entry);
                    }}
                  >
                    {t('wl.composer.history.replay')}
                  </button>
                  <button
                    type="button"
                    className={styles.button}
                    disabled={deleting}
                    aria-label={t('wl.composer.history.deleteItem', { method, target })}
                    onClick={() => {
                      onDelete(entry);
                    }}
                  >
                    {t('wl.composer.history.delete')}
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
