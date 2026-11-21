import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { formatByteSize } from '../../../components/format/format';
import { BoundedText } from './BoundedText';
import styles from './Tester.module.css';
import type { FinalResponse } from './types';

export interface FinalResponsePanelProps {
  final: FinalResponse;
  ruleCount: number;
  error: string | null;
}

export function FinalResponsePanel({ final, ruleCount, error }: FinalResponsePanelProps) {
  const { t, i18n } = useTranslation();
  const [viewing, setViewing] = useState(false);
  const detail = final.detail;
  const size = final.bodyBytes === null ? null : formatByteSize(final.bodyBytes, i18n.language);
  const byRule =
    final.groupIndex !== null && ruleCount > 1 ? t('wl.tester.final.byRule', { position: final.groupIndex + 1 }) : '';

  if (error !== null) {
    return (
      <section className={styles.section} aria-label={t('wl.tester.final.title')}>
        <p role="alert" className={styles.error}>
          {t('wl.tester.final.failed', { error })}
        </p>
      </section>
    );
  }

  return (
    <section className={styles.section} aria-label={t('wl.tester.final.title')}>
      <div className={styles.sectionHead}>
        {final.status === null ? (
          <p className={styles.final}>{t('wl.tester.final.configured')}</p>
        ) : (
          <p className={styles.final}>
            {size === null
              ? t('wl.tester.final.gets', { status: final.status, byRule })
              : t('wl.tester.final.getsSized', { status: final.status, size, byRule })}
          </p>
        )}
        {final.status === null ? null : (
          <button
            type="button"
            className={`${styles.button} ${styles.small}`}
            aria-expanded={viewing}
            onClick={() => {
              setViewing((value) => !value);
            }}
          >
            {viewing ? t('wl.tester.output.hide') : t('wl.tester.output.view')}
          </button>
        )}
      </div>
      {viewing ? (
        <div className={styles.section}>
          {final.contentType === null || final.contentType === '' ? null : (
            <p className={styles.summaryText}>{t('wl.tester.final.contentType', { value: final.contentType })}</p>
          )}
          {detail === null ? (
            <p className={styles.note}>
              {final.headerCount === null
                ? t('wl.tester.final.noDetail')
                : t('wl.tester.final.headerCount', { count: final.headerCount })}
            </p>
          ) : (
            <>
              {detail.headers.length === 0 ? (
                <p className={styles.note}>{t('wl.tester.final.noHeaders')}</p>
              ) : (
                <ul className={styles.rows} aria-label={t('wl.tester.final.headers')}>
                  {detail.headers.map((header, index) => (
                    <li key={`${header.name}:${index}`} className={styles.row}>
                      <span className={styles.path}>{header.name}</span>
                      <span className={styles.value}>{header.value}</span>
                    </li>
                  ))}
                </ul>
              )}
              {detail.body.encoding === 'base64' ? (
                <p className={styles.note}>
                  {t('wl.tester.final.binary', { size: formatByteSize(detail.bodySize, i18n.language) })}
                </p>
              ) : detail.body.data === '' ? (
                <p className={styles.note}>{t('wl.tester.final.emptyBody')}</p>
              ) : (
                <BoundedText text={detail.body.data} label={t('wl.tester.final.body')} />
              )}
              {detail.body.truncated ? <p className={styles.note}>{t('wl.tester.final.truncated')}</p> : null}
            </>
          )}
        </div>
      ) : null}
    </section>
  );
}
