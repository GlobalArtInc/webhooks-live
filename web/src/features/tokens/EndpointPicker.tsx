import { useTranslation } from 'react-i18next';
import { flattenOwnedEndpoints, useOwnedEndpoints } from '../../api/tokens';
import { shortEndpointId } from '../endpoints/format';
import { MAX_ENDPOINTS } from './draft';
import styles from './EndpointPicker.module.css';

export interface EndpointPickerProps {
  selected: readonly string[];
  describedBy?: string;
  onToggle: (endpointId: string, on: boolean) => void;
}

export function EndpointPicker({ selected, describedBy, onToggle }: EndpointPickerProps) {
  const { t } = useTranslation();
  const query = useOwnedEndpoints(true);
  const endpoints = flattenOwnedEndpoints(query.data);
  const full = selected.length >= MAX_ENDPOINTS;

  if (query.isPending) {
    return (
      <p role="status" className={styles.note}>
        {t('wl.tokens.picker.loading')}
      </p>
    );
  }
  if (query.isError && query.data === undefined) {
    return (
      <div role="alert" className={styles.note}>
        <p className={styles.message}>{t('wl.tokens.picker.failed')}</p>
        <button type="button" className={styles.button} onClick={() => void query.refetch()}>
          {t('wl.tokens.retry')}
        </button>
      </div>
    );
  }
  if (endpoints.length === 0) {
    return <p className={styles.note}>{t('wl.tokens.picker.empty')}</p>;
  }

  return (
    <div className={styles.root}>
      <ul className={styles.list} aria-label={t('wl.tokens.picker.label')} aria-describedby={describedBy}>
        {endpoints.map((endpoint) => {
          const checked = selected.includes(endpoint.id);
          const name = endpoint.title ?? t('wl.tokens.picker.untitled');
          return (
            <li key={endpoint.id}>
              <label className={styles.item}>
                <input
                  type="checkbox"
                  className={styles.checkbox}
                  checked={checked}
                  disabled={full && !checked}
                  onChange={(event) => onToggle(endpoint.id, event.target.checked)}
                />
                <span className={styles.name}>{name}</span>
                <code className={styles.id}>{shortEndpointId(endpoint.id)}</code>
              </label>
            </li>
          );
        })}
      </ul>
      {query.hasNextPage && (
        <button
          type="button"
          className={styles.button}
          disabled={query.isFetchingNextPage}
          onClick={() => void query.fetchNextPage()}
        >
          {t('wl.tokens.picker.more')}
        </button>
      )}
      <p className={styles.count} role="status">
        {t('wl.tokens.picker.count', { count: selected.length, max: MAX_ENDPOINTS })}
      </p>
    </div>
  );
}
