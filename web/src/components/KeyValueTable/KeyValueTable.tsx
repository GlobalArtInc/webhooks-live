import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { NameValue } from '../../api/types';
import { CopyButton } from '../CopyButton/CopyButton';
import styles from './KeyValueTable.module.css';

export interface KeyValueTableProps {
  label: string;
  rows: readonly NameValue[];
  rawRows?: readonly NameValue[];
  collapseAfter?: number;
  copyable?: boolean;
}

export function KeyValueTable({ label, rows, rawRows, collapseAfter, copyable = false }: KeyValueTableProps) {
  const { t } = useTranslation();
  const [raw, setRaw] = useState(false);
  const [expanded, setExpanded] = useState(false);

  const source = raw && rawRows !== undefined ? rawRows : rows;
  const limit = expanded ? Infinity : (collapseAfter ?? Infinity);
  const visible = source.slice(0, limit);
  const collapsible = collapseAfter !== undefined && source.length > collapseAfter;

  return (
    <div className={styles.wrap}>
      {rawRows !== undefined ? (
        <div className={styles.toolbar}>
          <button
            type="button"
            className={styles.toggle}
            aria-pressed={raw}
            onClick={() => {
              setRaw(!raw);
            }}
          >
            {t('wl.detail.keyValue.raw')}
          </button>
        </div>
      ) : null}
      {source.length === 0 ? (
        <p className={styles.empty}>{t('wl.detail.none')}</p>
      ) : (
        <div className={styles.scroller}>
          <table aria-label={label} className={styles.table}>
            <thead>
              <tr>
                <th scope="col">{t('wl.detail.keyValue.name')}</th>
                <th scope="col">{t('wl.detail.keyValue.value')}</th>
                {copyable ? (
                  <th scope="col" className={styles.copyColumn}>
                    <span className={styles.visuallyHidden}>{t('wl.detail.table.copyColumn')}</span>
                  </th>
                ) : null}
              </tr>
            </thead>
            <tbody>
              {visible.map((row, index) => (
                <tr key={index}>
                  <th scope="row" className={styles.name}>
                    {row.name}
                  </th>
                  <td className={styles.value}>{row.value}</td>
                  {copyable ? (
                    <td className={styles.copyColumn}>
                      <CopyButton
                        value={`${row.name}: ${row.value}`}
                        label={t('wl.detail.keyValue.copyRow', { name: row.name })}
                      />
                    </td>
                  ) : null}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {collapsible ? (
        <div className={styles.toolbar}>
          <button
            type="button"
            className={styles.toggle}
            aria-expanded={expanded}
            onClick={() => {
              setExpanded(!expanded);
            }}
          >
            {expanded ? t('wl.detail.keyValue.showFewer') : t('wl.detail.keyValue.showAll', { count: source.length })}
          </button>
        </div>
      ) : null}
    </div>
  );
}
