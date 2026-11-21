import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { NameValue } from '../../api/types';
import { HEADER_PREVIEW_ROWS, MAX_FIELD_CHARS, MAX_HEADERS, MAX_NAME_CHARS } from './constants';
import styles from './EmailDetail.module.css';
import { clampText, unfoldHeader } from './inert';
import { InertText } from './InertText';

export interface HeadersTableProps {
  rows: readonly NameValue[];
}

function normalize(rows: readonly NameValue[]): NameValue[] {
  return rows.slice(0, MAX_HEADERS).map((row) => ({
    name: clampText(String(row.name), MAX_NAME_CHARS),
    value: clampText(unfoldHeader(String(row.value)), MAX_FIELD_CHARS),
  }));
}

export function HeadersTable({ rows }: HeadersTableProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const normalized = useMemo(() => normalize(rows), [rows]);
  const visible = expanded ? normalized : normalized.slice(0, HEADER_PREVIEW_ROWS);
  const collapsible = normalized.length > HEADER_PREVIEW_ROWS;

  if (normalized.length === 0) {
    return <p className={styles.note}>{t('wl.email.none')}</p>;
  }
  return (
    <div className={styles.stack}>
      <div className={styles.scroller}>
        <table aria-label={t('wl.email.headers.table')} className={styles.dataTable}>
          <thead>
            <tr>
              <th scope="col">{t('wl.email.headers.name')}</th>
              <th scope="col">{t('wl.email.headers.value')}</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((row, index) => (
              <tr key={index}>
                <th scope="row" className={styles.nameCell}>
                  <InertText value={row.name} />
                </th>
                <td className={styles.mono}>
                  <InertText value={row.value} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {collapsible ? (
        <div>
          <button
            type="button"
            className={styles.action}
            aria-expanded={expanded}
            onClick={() => {
              setExpanded(!expanded);
            }}
          >
            {expanded ? t('wl.email.headers.showFewer') : t('wl.email.headers.showAll', { count: normalized.length })}
          </button>
        </div>
      ) : null}
      {rows.length > MAX_HEADERS ? (
        <p className={styles.note}>{t('wl.email.headers.capped', { count: MAX_HEADERS })}</p>
      ) : null}
    </div>
  );
}
