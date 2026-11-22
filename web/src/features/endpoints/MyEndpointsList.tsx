import { useId, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useInstance } from '../../api/queries';
import { latestTimestamp } from '../../components/format/useSharedNow';
import { EndpointRow } from './EndpointRow';
import { useNow } from './useNow';
import { byRecentlyOpened } from './storage';
import { useMyEndpoints } from './store';
import { captureUrlOf } from './format';
import { useCachedEndpointInfos } from './titleCache';
import styles from './MyEndpointsList.module.css';

export function MyEndpointsList() {
  const { t, i18n } = useTranslation();
  const endpoints = useMyEndpoints();
  const now = useNow(latestTimestamp(endpoints.map((entry) => entry.lastOpened)));
  const headingId = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const sorted = byRecentlyOpened(endpoints);
  const cachedInfos = useCachedEndpointInfos(sorted.map((entry) => entry.id));
  const publicUrl = useInstance().data?.public_url ?? null;

  return (
    <section aria-labelledby={headingId} className={styles.section}>
      <h2 id={headingId} ref={headingRef} tabIndex={-1} className={styles.heading}>
        {t('wl.home.list.heading')}
      </h2>
      {sorted.length === 0 ? (
        <p className={styles.empty}>{t('wl.home.list.empty')}</p>
      ) : (
        <ul className={styles.list}>
          {sorted.map((entry, index) => (
            <EndpointRow
              key={entry.id}
              entry={entry}
              now={now}
              locale={i18n.language}
              cached={cachedInfos[index] ?? null}
              captureUrl={captureUrlOf(entry.id, cachedInfos[index] ?? null, publicUrl)}
              onRemoved={() => headingRef.current?.focus()}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
