import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { MethodBadge } from '../../components/MethodBadge/MethodBadge';
import type { Reference } from './reference';
import styles from './ReferenceNav.module.css';

export interface ReferenceNavProps {
  reference: Reference;
  query: string;
  onNavigate: () => void;
}

export function ReferenceNav({ reference, query, onNavigate }: ReferenceNavProps) {
  const { t } = useTranslation();
  const search = query.trim() === '' ? {} : { q: query.trim() };
  return (
    <nav aria-label={t('wl.apireference.nav.label')} className={styles.nav}>
      {reference.tags.length === 0 ? (
        <p role="status" className={styles.empty}>
          {t('wl.apireference.nav.empty')}
        </p>
      ) : null}
      {reference.tags.map((tag, index) => {
        const name = tag.name ?? t('wl.apireference.nav.untagged');
        return (
          <div key={tag.name ?? ''} role="group" aria-labelledby={`nav-tag-${index}`} className={styles.group}>
            <p id={`nav-tag-${index}`} className={styles.tag}>
              {name}
              <span className={styles.count}>
                {t('wl.apireference.nav.operations', { count: tag.operations.length })}
              </span>
            </p>
            <ul className={styles.list}>
              {tag.operations.map((operation) => (
                <li key={operation.id}>
                  <Link
                    to="/docs/api/$operationId"
                    params={{ operationId: operation.id }}
                    search={search}
                    className={styles.item}
                    activeProps={{ 'aria-current': 'page' }}
                    onClick={onNavigate}
                  >
                    <MethodBadge method={operation.method} />{' '}
                    <span className={styles.itemText}>
                      <span className={styles.itemPath}>{operation.path}</span>
                      {operation.summary === null ? null : (
                        <span className={styles.itemSummary}>{operation.summary}</span>
                      )}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </nav>
  );
}
