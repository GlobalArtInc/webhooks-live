import { useTranslation } from 'react-i18next';
import { formatRelativeTime } from '../../components/format/format';
import { useLanguage } from '../../i18n/provider';
import { useNow } from '../endpoints/useNow';
import type { TokenErrorInfo } from './errors';
import { retryTime } from './errors';
import styles from './TokenErrorNotice.module.css';

export interface TokenErrorNoticeProps {
  error: TokenErrorInfo;
}

export function TokenErrorNotice({ error }: TokenErrorNoticeProps) {
  const { t } = useTranslation();
  const { language } = useLanguage();
  const now = useNow();
  const resumeAt = retryTime(error, now);

  let message: string;
  switch (error.kind) {
    case 'limit':
      message = t('wl.tokens.error.limit', { max: error.max });
      break;
    case 'rate':
      message =
        resumeAt === null
          ? t('wl.tokens.error.rateSoon', { max: error.max })
          : t('wl.tokens.error.rate', {
              max: error.max,
              when: formatRelativeTime(new Date(resumeAt).toISOString(), now, language),
            });
      break;
    case 'session':
      message = t('wl.tokens.error.session');
      break;
    case 'fields':
      message = t('wl.tokens.error.fields');
      break;
    case 'generic':
      message = t('wl.tokens.error.generic');
      break;
  }

  return (
    <div role="alert" className={styles.notice}>
      <p className={styles.message}>{message}</p>
      {error.kind === 'fields' && (
        <ul className={styles.fields}>
          {error.fields.map((field) => (
            <li key={`${field.field}:${field.message}`}>
              <code>{field.field}</code> <span lang="en">{field.message}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
