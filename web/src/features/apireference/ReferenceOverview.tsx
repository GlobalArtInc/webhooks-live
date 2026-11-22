import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { SecurityChips } from './SecurityChips';
import type { Reference } from './reference';
import { TextBlocks } from './TextBlocks';
import styles from './ReferenceOverview.module.css';

export interface ReferenceOverviewProps {
  reference: Reference;
  baseUrl: string;
  query: string;
}

export function ReferenceOverview({ reference, baseUrl, query }: ReferenceOverviewProps) {
  const { t } = useTranslation();
  const search = query.trim() === '' ? {} : { q: query.trim() };
  return (
    <header className={styles.overview}>
      <h1 className={styles.title}>{reference.title ?? t('wl.apireference.title')}</h1>
      {reference.version === null ? null : (
        <p className={styles.version}>{t('wl.apireference.version', { version: reference.version })}</p>
      )}
      <div className={styles.description}>
        <TextBlocks text={reference.description} />
      </div>
      <dl className={styles.facts}>
        <dt>{t('wl.apireference.overview.server')}</dt>
        <dd>
          <code className={styles.base}>{baseUrl}</code>
        </dd>
      </dl>
      {reference.securitySchemes.length === 0 ? null : (
        <section className={styles.section} aria-labelledby="apiref-auth">
          <h2 id="apiref-auth" className={styles.heading}>
            {t('wl.apireference.overview.auth')}
          </h2>
          <ul className={styles.schemes}>
            {reference.securitySchemes.map((scheme) => (
              <li key={scheme.name} className={styles.scheme}>
                <SecurityChips chips={[scheme]} />
                <div className={styles.schemeText}>
                  <TextBlocks text={scheme.description} />
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
      {reference.tags.length === 0 ? null : (
        <section className={styles.section} aria-labelledby="apiref-tags">
          <h2 id="apiref-tags" className={styles.heading}>
            {t('wl.apireference.overview.tags')}
          </h2>
          <ul className={styles.tags}>
            {reference.tags.map((tag) => {
              const [first] = tag.operations;
              const name = tag.name ?? t('wl.apireference.nav.untagged');
              return (
                <li key={tag.name ?? ''} className={styles.tag}>
                  {first === undefined ? (
                    <span className={styles.tagName}>{name}</span>
                  ) : (
                    <Link
                      to="/docs/api/$operationId"
                      params={{ operationId: first.id }}
                      search={search}
                      className={styles.tagName}
                    >
                      {name}
                    </Link>
                  )}
                  <span className={styles.tagCount}>
                    {t('wl.apireference.nav.operations', { count: tag.operations.length })}
                  </span>
                  <span className={styles.tagText}>
                    <TextBlocks text={tag.description} />
                  </span>
                </li>
              );
            })}
          </ul>
        </section>
      )}
    </header>
  );
}
