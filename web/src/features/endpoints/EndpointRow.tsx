import { Link } from '@tanstack/react-router';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { EndpointLabelEditor } from './EndpointLabelEditor';
import { endpointName, formatRelativeTime, shortEndpointId } from './format';
import { forgetEndpoint, markEndpointOpened, renameMyEndpoint } from './store';
import type { MyEndpoint } from './storage';
import styles from './EndpointRow.module.css';

export interface EndpointRowProps {
  entry: MyEndpoint;
  now: number;
  locale: string;
  cachedTitle: string | null;
  onRemoved: () => void;
}

export function EndpointRow({ entry, now, locale, cachedTitle, onRemoved }: EndpointRowProps) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const renameRef = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef(false);
  const name = endpointName(entry, cachedTitle);

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
      <li className={styles.row}>
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
    <li className={styles.row}>
      <div className={styles.identity}>
        <span className={styles.name} title={name}>
          {name}
        </span>
        {name !== shortEndpointId(entry.id) && <span className={styles.id}>{shortEndpointId(entry.id)}</span>}
        <span className={styles.meta}>
          {t('wl.home.list.lastOpened', { time: formatRelativeTime(entry.lastOpened, now, locale) })}
        </span>
      </div>
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
