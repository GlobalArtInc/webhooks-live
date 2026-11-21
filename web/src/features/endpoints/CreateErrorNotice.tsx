import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import styles from './CreateErrorNotice.module.css';

export interface CreateErrorNoticeProps {
  error: Error;
  onRetry: () => void;
}

export function isValidationFailure(error: Error | null): boolean {
  return error instanceof ApiError && error.status === 422 && error.details.length > 0;
}

export function CreateErrorNotice({ error, onRetry }: CreateErrorNoticeProps) {
  const { t } = useTranslation();
  const retryAfter = error instanceof ApiError ? error.retryAfter : null;

  let message: string;
  if (error instanceof ApiError && error.status === 429) {
    message =
      retryAfter === null
        ? t('wl.home.error.rateLimited')
        : t('wl.home.error.rateLimitedWait', { seconds: retryAfter });
  } else if (error instanceof ApiError && error.status >= 500) {
    message = t('wl.home.error.server');
  } else {
    message = t('wl.home.error.failed');
  }

  return (
    <div role="alert" tabIndex={-1} autoFocus className={styles.notice}>
      <p className={styles.message}>{message}</p>
      <button type="button" className={styles.retry} onClick={onRetry}>
        {t('wl.home.error.retry')}
      </button>
    </div>
  );
}
