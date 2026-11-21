import { useId, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { EndpointRow } from './EndpointRow';
import { useNow } from './useNow';
import { byRecentlyOpened } from './storage';
import { useMyEndpoints } from './store';
import styles from './MyEndpointsList.module.css';

export function MyEndpointsList() {
  const { t, i18n } = useTranslation();
  const endpoints = useMyEndpoints();
  const now = useNow();
  const headingId = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const sorted = byRecentlyOpened(endpoints);

  return (
    <section aria-labelledby={headingId} className={styles.section}>
      <h2 id={headingId} ref={headingRef} tabIndex={-1} className={styles.heading}>
        {t('wl.home.list.heading')}
      </h2>
      {sorted.length === 0 ? (
        <p className={styles.empty}>{t('wl.home.list.empty')}</p>
      ) : (
        <ul className={styles.list}>
          {sorted.map((entry) => (
            <EndpointRow
              key={entry.id}
              entry={entry}
              now={now}
              locale={i18n.language}
              onRemoved={() => headingRef.current?.focus()}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
