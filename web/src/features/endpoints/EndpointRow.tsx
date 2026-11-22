import { Link } from '@tanstack/react-router';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../components/CopyButton/CopyButton';
import { MiddleTruncate } from '../../components/MiddleTruncate/MiddleTruncate';
import { EndpointLabelEditor } from './EndpointLabelEditor';
import { endpointName, formatRelativeTime, shortEndpointId } from './format';
import { forgetEndpoint, markEndpointOpened, renameMyEndpoint } from './store';
import type { MyEndpoint } from './storage';
import type { CachedEndpointInfo } from './titleCache';
import styles from './EndpointRow.module.css';

export interface EndpointRowProps {
  entry: MyEndpoint;
  now: number;
  locale: string;
  cached: CachedEndpointInfo | null;
  captureUrl: string | null;
  onRemoved: () => void;
}

export function EndpointRow({ entry, now, locale, cached, captureUrl, onRemoved }: EndpointRowProps) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const renameRef = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef(false);
  const name = endpointName(entry, cached?.title ?? null);

  useEffect(() => {
    if (!editing && returnFocus.current) {
      returnFocus.current = false;
      renameRef.current?.focus();
    }
  }, [editing]);

  function closeEditor() {
    returnFocus.current = true;
    setEditing(false);
  }

  if (editing) {
    return (
      <li className={styles.card}>
        <EndpointLabelEditor
          name={name}
          initial={entry.label ?? ''}
          onSave={(label) => {
            renameMyEndpoint(entry.id, label);
            closeEditor();
          }}
          onCancel={closeEditor}
        />
      </li>
    );
  }

  return (
    <li className={styles.card}>
      <div className={styles.identity}>
        <span className={styles.name} title={name}>
          {name}
        </span>
        {name !== shortEndpointId(entry.id) && <span className={styles.id}>{shortEndpointId(entry.id)}</span>}
      </div>
      {captureUrl === null ? null : (
        <div className={styles.urlRow}>
          <code className={styles.url}>
            <MiddleTruncate text={captureUrl} />
          </code>
          <CopyButton
            size="large"
            value={captureUrl}
            label={t('wl.home.list.copyUrlLabel', { name })}
            idleText={t('wl.home.list.copyUrl')}
          />
        </div>
      )}
      <p className={styles.meta}>
        <span>{t('wl.home.list.created', { time: formatRelativeTime(entry.added, now, locale) })}</span>
        <span>{t('wl.home.list.lastOpened', { time: formatRelativeTime(entry.lastOpened, now, locale) })}</span>
        {cached === null ? null : <span>{t('wl.home.list.requests', { count: cached.captureCount })}</span>}
      </p>
      <div className={styles.actions}>
        <Link
          to="/e/$id"
          params={{ id: entry.id }}
          className={styles.action}
          aria-label={t('wl.home.list.openLabel', { name })}
          onClick={() => markEndpointOpened(entry.id)}
        >
          {t('wl.home.list.open')}
        </Link>
        <button
          ref={renameRef}
          type="button"
          className={styles.action}
          aria-label={t('wl.home.list.renameLabel', { name })}
          onClick={() => setEditing(true)}
        >
          {t('wl.home.list.rename')}
        </button>
        <button
          type="button"
          className={styles.action}
          aria-label={t('wl.home.list.removeLabel', { name })}
          onClick={() => {
            forgetEndpoint(entry.id);
            onRemoved();
          }}
        >
          {t('wl.home.list.remove')}
        </button>
      </div>
    </li>
  );
}
