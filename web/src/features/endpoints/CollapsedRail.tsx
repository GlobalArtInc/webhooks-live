import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { ChevronsRight, Folder, List, Plus, Send, Tag } from '../../components/icons';
import { RailTip } from '../../components/RailTip/RailTip';
import { markEndpointOpened } from './store';
import { rowName, type OrganizedEndpoint } from '../folders/model';
import { useOrganizeData } from '../folders/useOrganizeData';
import { CreateErrorNotice } from './CreateErrorNotice';
import styles from './CollapsedRail.module.css';

const MAX_AVATARS = 8;
const AVATAR_CHARACTERS = 2;
const NO_FILTER = {};

export interface CollapsedRailProps {
  headingId: string;
  listId: string;
  creating: boolean;
  createError: Error | null;
  onCreate: () => void;
  onExpand: () => void;
}

export function avatarText(row: OrganizedEndpoint): string {
  const source = row.label === null || row.label.trim() === '' ? row.id : row.label.trim();
  return Array.from(source).slice(0, AVATAR_CHARACTERS).join('').toUpperCase();
}

export function CollapsedRail({ headingId, listId, creating, createError, onCreate, onExpand }: CollapsedRailProps) {
  const { t } = useTranslation();
  const { allRows } = useOrganizeData(NO_FILTER);
  const shown = allRows.slice(0, MAX_AVATARS);
  const hidden = allRows.length - shown.length;
  const expandLabel = t('wl.home.rail.expandLabel');
  const createLabel = t('wl.home.rail.newLabel');

  return (
    <>
      <h2 id={headingId} className={styles.srOnly}>
        {t('wl.home.rail.heading')}
      </h2>
      <RailTip label={expandLabel}>
        <button type="button" className={styles.item} aria-expanded="false" aria-controls={listId} onClick={onExpand}>
          <ChevronsRight />
          <span className={styles.srOnly}>{expandLabel}</span>
        </button>
      </RailTip>
      <RailTip label={createLabel}>
        <button type="button" className={styles.create} disabled={creating} onClick={onCreate}>
          <Plus />
          <span className={styles.srOnly}>{createLabel}</span>
        </button>
      </RailTip>
      {createError !== null && !creating && <CreateErrorNotice error={createError} onRetry={onCreate} />}
      <RailTip label={t('wl.home.rail.heading')}>
        <Link to="/endpoints" className={styles.item} activeProps={{ className: styles.active }}>
          <List />
          <span className={styles.srOnly}>{t('wl.home.rail.heading')}</span>
        </Link>
      </RailTip>
      <RailTip label={t('wl.composer.rail')}>
        <Link to="/send" className={styles.item} activeProps={{ className: styles.active }}>
          <Send />
          <span className={styles.srOnly}>{t('wl.composer.rail')}</span>
        </Link>
      </RailTip>
      <RailTip label={t('wl.rail.folders.heading')}>
        <button type="button" className={styles.item} onClick={onExpand}>
          <Folder />
          <span className={styles.srOnly}>{t('wl.rail.folders.heading')}</span>
        </button>
      </RailTip>
      <RailTip label={t('wl.rail.labels.heading')}>
        <button type="button" className={styles.item} onClick={onExpand}>
          <Tag />
          <span className={styles.srOnly}>{t('wl.rail.labels.heading')}</span>
        </button>
      </RailTip>
      {shown.length === 0 ? null : (
        <>
          <hr className={styles.divider} />
          {shown.map((row) => (
            <RailTip key={row.id} label={rowName(row)}>
              <Link
                to="/e/$id"
                params={{ id: row.id }}
                className={styles.item}
                activeProps={{ className: styles.active }}
                onClick={() => {
                  if (row.source === 'local') {
                    markEndpointOpened(row.id);
                  }
                }}
              >
                <span className={styles.avatar} aria-hidden="true">
                  {avatarText(row)}
                </span>
                <span className={styles.srOnly}>{rowName(row)}</span>
              </Link>
            </RailTip>
          ))}
          {hidden > 0 && (
            <RailTip label={t('wl.home.rail.more')}>
              <button type="button" className={styles.item} onClick={onExpand}>
                <span className={styles.more} aria-hidden="true">
                  +{hidden}
                </span>
                <span className={styles.srOnly}>{t('wl.home.rail.more')}</span>
              </button>
            </RailTip>
          )}
        </>
      )}
    </>
  );
}
