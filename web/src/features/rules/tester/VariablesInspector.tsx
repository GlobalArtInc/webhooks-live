import type { TFunction } from 'i18next';
import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../../components/CopyButton/CopyButton';
import { VALUE_PREVIEW_CHARS } from './constants';
import styles from './Tester.module.css';
import { filterRows } from './variables';
import type { VariableGroup, VariableRow } from './types';

export interface VariablesInspectorProps {
  rows: readonly VariableRow[];
  position: number;
  onInsert: ((path: string) => void) | undefined;
}

const GROUPS: readonly VariableGroup[] = ['request', 'var', 'store', 'counter'];

interface RowViewProps {
  row: VariableRow;
  expanded: boolean;
  onToggle: (path: string) => void;
  onInsert: ((path: string) => void) | undefined;
}

function RowView({ row, expanded, onToggle, onInsert }: RowViewProps) {
  const { t } = useTranslation();
  const long = row.text.length > VALUE_PREVIEW_CHARS;
  const shown = long && !expanded ? `${row.text.slice(0, VALUE_PREVIEW_CHARS)}...` : row.text;
  return (
    <li className={styles.row}>
      <span className={styles.marker} data-change={row.change} aria-hidden="true">
        {row.change === 'new' ? '+' : row.change === 'changed' ? '~' : ' '}
      </span>
      <span className={styles.path}>{row.path}</span>
      {row.known ? (
        <span className={styles.value}>{shown}</span>
      ) : (
        <span className={styles.value}>{t('wl.tester.variables.unknown')}</span>
      )}
      {row.change === 'same' ? null : (
        <span className={styles.entryStatus}>
          {row.change === 'new' ? t('wl.tester.variables.new') : t('wl.tester.variables.changed')}
        </span>
      )}
      {long ? (
        <button
          type="button"
          className={`${styles.button} ${styles.small}`}
          aria-expanded={expanded}
          aria-label={
            expanded
              ? t('wl.tester.variables.collapse', { path: row.path })
              : t('wl.tester.variables.expand', { path: row.path })
          }
          onClick={() => {
            onToggle(row.path);
          }}
        >
          {expanded ? t('wl.tester.variables.less') : t('wl.tester.variables.more')}
        </button>
      ) : null}
      <CopyButton value={row.path} label={t('wl.tester.variables.copy', { path: row.path })} />
      {onInsert === undefined ? null : (
        <button
          type="button"
          className={`${styles.button} ${styles.small}`}
          aria-label={t('wl.tester.variables.insert', { path: row.path })}
          onClick={() => {
            onInsert(row.path);
          }}
        >
          {t('wl.tester.variables.insertShort')}
        </button>
      )}
    </li>
  );
}

function groupTitle(group: VariableGroup, t: TFunction): string {
  switch (group) {
    case 'request':
      return t('wl.tester.variables.group.request');
    case 'var':
      return t('wl.tester.variables.group.var');
    case 'store':
      return t('wl.tester.variables.group.store');
    case 'counter':
      return t('wl.tester.variables.group.counter');
  }
}

export function VariablesInspector({ rows, position, onInsert }: VariablesInspectorProps) {
  const { t } = useTranslation();
  const filterId = useId();
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const visible = filterRows(rows, query);

  function toggle(path: string): void {
    setOpen((current) => {
      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }

  return (
    <section className={styles.section} aria-labelledby={`${filterId}-title`}>
      <div className={styles.sectionHead}>
        <h3 id={`${filterId}-title`} className={styles.sectionTitle}>
          {t('wl.tester.variables.title', { position })}
        </h3>
        <label htmlFor={filterId} className={styles.note}>
          {t('wl.tester.variables.filter')}
        </label>
        <input
          id={filterId}
          type="search"
          className={styles.field}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
        />
      </div>
      {visible.length === 0 ? (
        <p className={styles.note}>
          {rows.length === 0 ? t('wl.tester.variables.empty') : t('wl.tester.variables.noMatch')}
        </p>
      ) : (
        GROUPS.map((group) => {
          const members = visible.filter((row) => row.group === group);
          return members.length === 0 ? null : (
            <div key={group}>
              <h4 className={styles.sectionTitle}>{groupTitle(group, t)}</h4>
              <ul className={styles.rows}>
                {members.map((row) => (
                  <RowView
                    key={row.path}
                    row={row}
                    expanded={open.has(row.path)}
                    onToggle={toggle}
                    onInsert={onInsert}
                  />
                ))}
              </ul>
            </div>
          );
        })
      )}
    </section>
  );
}
