import { Link } from '@tanstack/react-router';
import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { useRailState } from '../../app/shell/railState';
import { CreateErrorNotice } from './CreateErrorNotice';
import { endpointName } from './format';
import { markEndpointOpened, useMyEndpoints } from './store';
import { byRecentlyOpened } from './storage';
import { useCreateAndOpen } from './useCreateAndOpen';
import styles from './EndpointRail.module.css';

export function EndpointRail() {
  const { t } = useTranslation();
  const endpoints = useMyEndpoints();
  const { create, isPending, error } = useCreateAndOpen();
  const { collapsed, toggleCollapsed, closeDrawer } = useRailState();
  const headingId = useId();
  const listId = useId();
  const sorted = byRecentlyOpened(endpoints);

  return (
    <nav aria-labelledby={headingId} className={styles.rail}>
      <div className={styles.head}>
        <h2 id={headingId} className={styles.heading}>
          {t('wl.home.rail.heading')}
        </h2>
        <button
          type="button"
          className={styles.toggle}
          aria-expanded={!collapsed}
          aria-controls={listId}
          onClick={toggleCollapsed}
        >
          {collapsed ? t('wl.home.rail.expand') : t('wl.home.rail.collapse')}
        </button>
      </div>
      <button
        type="button"
        className={styles.create}
        aria-label={t('wl.home.rail.newLabel')}
        disabled={isPending}
        onClick={() => create({})}
      >
        {isPending ? t('wl.home.create.busy') : t('wl.home.rail.new')}
      </button>
      {error !== null && !isPending && <CreateErrorNotice error={error} onRetry={() => create({})} />}
      {sorted.length === 0 && <p className={styles.empty}>{t('wl.home.rail.empty')}</p>}
      <ul id={listId} className={styles.list}>
        {sorted.map((entry) => (
          <li key={entry.id}>
            <Link
              to="/e/$id"
              params={{ id: entry.id }}
              className={styles.link}
              activeProps={{ className: styles.active }}
              onClick={() => {
                markEndpointOpened(entry.id);
                closeDrawer();
              }}
            >
              {endpointName(entry)}
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  );
}
