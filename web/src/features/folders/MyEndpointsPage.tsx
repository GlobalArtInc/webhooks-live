import { useNavigate } from '@tanstack/react-router';
import { useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { latestTimestamp } from '../../components/format/useSharedNow';
import { useNow } from '../endpoints/useNow';
import { EndpointCard } from './EndpointCard';
import { filterOrganized, labelCounts, type FolderView, type SourceFilter } from './model';
import { serverQueryFor, type EndpointsSearch } from './search';
import { useOrganizeData } from './useOrganizeData';
import styles from './MyEndpointsPage.module.css';

export interface MyEndpointsPageProps {
  search: EndpointsSearch;
}

function sourceFilterOf(value: string): SourceFilter | undefined {
  return value === 'local' || value === 'account' || value === 'shared' ? value : undefined;
}

function folderOptionsOf(folders: readonly FolderView[], source: FolderView['source']) {
  return folders
    .filter((folder) => folder.source === source)
    .map((folder) => (
      <option key={folder.key} value={folder.key}>
        {folder.name}
      </option>
    ));
}

export function MyEndpointsPage({ search }: MyEndpointsPageProps) {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const headingId = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const [notice, setNotice] = useState('');
  const data = useOrganizeData(serverQueryFor(search));
  const now = useNow(latestTimestamp(data.allRows.map((row) => row.lastActivity)));
  const rows = filterOrganized(data.shownRows, search);
  const labels = labelCounts(data.allRows);
  const suggestions = labels.map((entry) => entry.label);
  const filtersActive = search.folder !== undefined || search.label !== undefined || search.source !== undefined;

  function setFilters(patch: EndpointsSearch) {
    void navigate({ to: '/endpoints', search: (previous) => ({ ...previous, ...patch }) });
  }

  function clearFilters() {
    void navigate({ to: '/endpoints', search: {} });
  }

  function settle(message: string) {
    setNotice(message);
    headingRef.current?.focus();
  }

  const localFolders = folderOptionsOf(data.folders, 'local');
  const accountFolders = folderOptionsOf(data.folders, 'account');

  return (
    <section className={styles.page} aria-labelledby={headingId}>
      <h1 id={headingId} ref={headingRef} tabIndex={-1} className={styles.title}>
        {t('wl.rail.page.heading')}
      </h1>
      <p className={styles.intro}>{t('wl.rail.page.intro')}</p>
      <div className={styles.filters} role="group" aria-label={t('wl.rail.page.filters.label')}>
        <label className={styles.control}>
          <span className={styles.controlLabel}>{t('wl.rail.page.filters.source')}</span>
          <select
            className={styles.select}
            value={search.source ?? ''}
            onChange={(event) => setFilters({ source: sourceFilterOf(event.target.value) })}
          >
            <option value="">{t('wl.rail.page.filters.allSources')}</option>
            <option value="local">{t('wl.rail.source.local')}</option>
            <option value="account">{t('wl.rail.source.account')}</option>
            <option value="shared">{t('wl.rail.source.shared')}</option>
          </select>
        </label>
        <label className={styles.control}>
          <span className={styles.controlLabel}>{t('wl.rail.page.filters.folder')}</span>
          <select
            className={styles.select}
            value={search.folder ?? ''}
            onChange={(event) => setFilters({ folder: event.target.value === '' ? undefined : event.target.value })}
          >
            <option value="">{t('wl.rail.page.filters.allFolders')}</option>
            <option value="unfiled">{t('wl.rail.folders.unfiled')}</option>
            {localFolders.length === 0 ? null : <optgroup label={t('wl.rail.source.local')}>{localFolders}</optgroup>}
            {accountFolders.length === 0 ? null : (
              <optgroup label={t('wl.rail.source.account')}>{accountFolders}</optgroup>
            )}
          </select>
        </label>
      </div>
      {labels.length === 0 ? null : (
        <div className={styles.labels} role="group" aria-label={t('wl.rail.labels.heading')}>
          {labels.map(({ label, count }) => (
            <button
              key={label}
              type="button"
              className={styles.labelToggle}
              aria-pressed={search.label === label}
              aria-label={t('wl.rail.labels.item', { label, count })}
              onClick={() => setFilters({ label: search.label === label ? undefined : label })}
            >
              {label}
            </button>
          ))}
        </div>
      )}
      {filtersActive ? (
        <button type="button" className={styles.clear} onClick={clearFilters}>
          {t('wl.rail.page.filters.clear')}
        </button>
      ) : null}
      <p className={styles.status} role="status">
        {notice}
      </p>
      {data.pending ? <p className={styles.empty}>{t('wl.rail.page.loading')}</p> : null}
      {data.failed ? (
        <p className={styles.error} role="alert">
          {t('wl.rail.page.accountError')}
        </p>
      ) : null}
      {rows.length === 0 && !data.pending ? (
        <p className={styles.empty}>{filtersActive ? t('wl.rail.page.emptyFiltered') : t('wl.rail.page.empty')}</p>
      ) : null}
      <ul className={styles.list}>
        {rows.map((row) => (
          <EndpointCard
            key={row.id}
            row={row}
            folders={data.folders}
            suggestions={suggestions}
            session={data.session}
            now={now}
            locale={i18n.language}
            onSettled={settle}
          />
        ))}
      </ul>
    </section>
  );
}
