import { useEffect, useId, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { StatePage } from '../../components/StatePage/StatePage';
import { operationAnchorId, operationHeadingId } from './anchors';
import { OPENAPI_URL, useOpenApiDocument } from './document';
import { filterReference } from './filter';
import { OperationView } from './OperationView';
import { buildReference } from './reference';
import { ReferenceNav } from './ReferenceNav';
import { ReferenceOverview } from './ReferenceOverview';
import type { SampleContext } from './samples';
import { TextBlocks } from './TextBlocks';
import styles from './ApiReferencePage.module.css';

const AUTO_OPEN_LIMIT = 5;

export interface ApiReferencePageProps {
  operationId: string | null;
  query: string;
  onQueryChange: (query: string) => void;
}

function focusOperation(operationId: string): void {
  const article = window.document.getElementById(operationAnchorId(operationId));
  if (article === null) {
    return;
  }
  article.scrollIntoView({ block: 'start' });
  window.document.getElementById(operationHeadingId(operationId))?.focus({ preventScroll: true });
}

export function ApiReferencePage({ operationId, query, onQueryChange }: ApiReferencePageProps) {
  const { t } = useTranslation();
  const loaded = useOpenApiDocument();
  const [navOpen, setNavOpen] = useState(false);
  const navId = useId();
  const reference = useMemo(() => (loaded.data === undefined ? null : buildReference(loaded.data)), [loaded.data]);
  const visible = useMemo(() => (reference === null ? null : filterReference(reference, query)), [reference, query]);
  const serverUrl = reference?.serverUrl ?? '';
  const context = useMemo<SampleContext>(() => ({ origin: window.location.origin, serverUrl }), [serverUrl]);
  const ready = reference !== null;

  useEffect(() => {
    if (operationId !== null && ready) {
      focusOperation(operationId);
    }
  }, [operationId, ready]);

  if (loaded.isError && loaded.data === undefined) {
    const notFound = loaded.error instanceof ApiError && loaded.error.status === 404;
    return (
      <StatePage
        kind={notFound ? 'not-found' : 'error'}
        detail={loaded.error instanceof ApiError ? { status: loaded.error.status } : {}}
        onRetry={() => void loaded.refetch()}
      />
    );
  }
  if (reference === null || visible === null) {
    return (
      <p role="status" className={styles.loading}>
        {t('wl.apireference.loading')}
      </p>
    );
  }

  const unknownOperation =
    operationId !== null && !reference.operations.some((operation) => operation.id === operationId);
  const autoOpenAll = visible.operations.length <= AUTO_OPEN_LIMIT;
  const baseUrl = `${context.origin}${reference.serverUrl}`;

  return (
    <div className={styles.page}>
      <aside className={styles.sidebar}>
        <div className={styles.sidebarHead}>
          <label className={styles.filterLabel} htmlFor={`${navId}-filter`}>
            {t('wl.apireference.nav.filterLabel')}
          </label>
          <input
            id={`${navId}-filter`}
            type="search"
            className={styles.filter}
            value={query}
            placeholder={t('wl.apireference.nav.filterPlaceholder')}
            onChange={(event) => {
              onQueryChange(event.target.value);
              setNavOpen(true);
            }}
          />
          <button
            type="button"
            className={styles.toggle}
            aria-expanded={navOpen}
            aria-controls={`${navId}-panel`}
            onClick={() => setNavOpen((current) => !current)}
          >
            {t('wl.apireference.nav.toggle')}
          </button>
        </div>
        <div id={`${navId}-panel`} className={styles.panel} data-open={navOpen}>
          <ReferenceNav reference={visible} query={query} onNavigate={() => setNavOpen(false)} />
          <a className={styles.download} href={OPENAPI_URL} download="openapi.json">
            {t('wl.apireference.download')}
          </a>
        </div>
      </aside>
      <div className={styles.content}>
        <ReferenceOverview reference={reference} baseUrl={baseUrl} query={query} />
        {unknownOperation ? (
          <p role="status" className={styles.missing}>
            {t('wl.apireference.operation.missing', { id: operationId })}
          </p>
        ) : null}
        {visible.tags.length === 0 ? (
          <p role="status" className={styles.empty}>
            {t('wl.apireference.nav.empty')}
          </p>
        ) : null}
        {visible.tags.map((tag, index) => (
          <div key={tag.name ?? ''} className={styles.tagSection}>
            <h2 id={`${navId}-tag-${index}`} className={styles.tagHeading}>
              {tag.name ?? t('wl.apireference.nav.untagged')}
            </h2>
            {tag.description === null ? null : (
              <div className={styles.tagDescription}>
                <TextBlocks text={tag.description} />
              </div>
            )}
            {tag.operations.map((operation) => (
              <OperationView
                key={operation.id}
                operation={operation}
                context={context}
                autoOpen={operation.id === operationId || autoOpenAll}
              />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
