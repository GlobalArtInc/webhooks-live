import { Link } from '@tanstack/react-router';
import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { useRailState } from '../../app/shell/railState';
import { FolderRail } from '../folders/FolderRail';
import { CollapsedRail } from './CollapsedRail';
import { CreateErrorNotice } from './CreateErrorNotice';
import { useCreateAndOpen } from './useCreateAndOpen';
import styles from './EndpointRail.module.css';

export function EndpointRail() {
  const { t } = useTranslation();
  const { create, isPending, error } = useCreateAndOpen();
  const { collapsed, toggleCollapsed } = useRailState();
  const headingId = useId();
  const listId = useId();

  const folders = (
    <div className={styles.folders} hidden={collapsed}>
      <FolderRail listId={listId} />
    </div>
  );

  if (collapsed) {
    return (
      <nav aria-labelledby={headingId} className={styles.rail} data-collapsed="true">
        <CollapsedRail
          headingId={headingId}
          listId={listId}
          creating={isPending}
          createError={error}
          onCreate={() => create({})}
          onExpand={toggleCollapsed}
        />
        {folders}
      </nav>
    );
  }

  return (
    <nav aria-labelledby={headingId} className={styles.rail}>
      <div className={styles.head}>
        <h2 id={headingId} className={styles.heading}>
          {t('wl.home.rail.heading')}
        </h2>
        <button
          type="button"
          className={styles.toggle}
          aria-label={t('wl.home.rail.collapseLabel')}
          aria-expanded="true"
          aria-controls={listId}
          onClick={toggleCollapsed}
        >
          {t('wl.home.rail.collapse')}
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
      <Link to="/send" className={styles.link} activeProps={{ className: styles.active }}>
        {t('wl.composer.rail')}
      </Link>
      {folders}
    </nav>
  );
}
