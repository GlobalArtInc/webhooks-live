import { useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  createHeaderRow,
  headerFieldPath,
  MAX_RESPONSE_HEADERS,
  type HeaderPart,
  type HeaderRow,
  type IssueMap,
} from './responseDraft';
import { useIssueText } from './issueText';
import styles from './HeadersEditor.module.css';

export interface HeadersEditorProps {
  rows: readonly HeaderRow[];
  issues: IssueMap;
  onChange: (rows: readonly HeaderRow[]) => void;
}

export function HeadersEditor({ rows, issues, onChange }: HeadersEditorProps) {
  const { t } = useTranslation();
  const issueText = useIssueText();
  const baseId = useId();
  const addRef = useRef<HTMLButtonElement>(null);
  const [freshKey, setFreshKey] = useState<number | null>(null);
  const full = rows.length >= MAX_RESPONSE_HEADERS;
  const hintId = `${baseId}-hint`;

  function addRow() {
    const row = createHeaderRow();
    setFreshKey(row.key);
    onChange([...rows, row]);
  }

  function removeRow(key: number) {
    onChange(rows.filter((row) => row.key !== key));
    addRef.current?.focus();
  }

  function editRow(key: number, part: HeaderPart, text: string) {
    onChange(
      rows.map((row) => {
        if (row.key !== key) {
          return row;
        }
        return part === 'name' ? { ...row, name: text } : { ...row, value: text };
      }),
    );
  }

  return (
    <fieldset className={styles.editor} aria-describedby={hintId}>
      <legend className={styles.legend}>{t('wl.home.headers.legend')}</legend>
      {rows.length > 0 && (
        <div className={styles.columns} aria-hidden="true">
          <span>{t('wl.home.headers.nameColumn')}</span>
          <span>{t('wl.home.headers.valueColumn')}</span>
        </div>
      )}
      {rows.map((row, index) => {
        const position = index + 1;
        const namePath = headerFieldPath(index, 'name');
        const valuePath = headerFieldPath(index, 'value');
        const nameIssue = issues.get(namePath);
        const valueIssue = issues.get(valuePath);
        return (
          <div key={row.key} className={styles.row}>
            <div className={styles.field}>
              <input
                id={`${baseId}-${index}-name`}
                type="text"
                className={styles.input}
                aria-label={t('wl.home.headers.nameLabel', { index: position })}
                aria-invalid={nameIssue === undefined ? undefined : true}
                aria-describedby={nameIssue === undefined ? undefined : `${baseId}-${index}-name-error`}
                autoFocus={row.key === freshKey}
                value={row.name}
                onChange={(event) => editRow(row.key, 'name', event.target.value)}
              />
              {nameIssue !== undefined && (
                <p id={`${baseId}-${index}-name-error`} className={styles.error}>
                  {issueText(namePath, nameIssue)}
                </p>
              )}
            </div>
            <div className={styles.field}>
              <input
                id={`${baseId}-${index}-value`}
                type="text"
                className={styles.input}
                aria-label={t('wl.home.headers.valueLabel', { index: position })}
                aria-invalid={valueIssue === undefined ? undefined : true}
                aria-describedby={valueIssue === undefined ? undefined : `${baseId}-${index}-value-error`}
                value={row.value}
                onChange={(event) => editRow(row.key, 'value', event.target.value)}
              />
              {valueIssue !== undefined && (
                <p id={`${baseId}-${index}-value-error`} className={styles.error}>
                  {issueText(valuePath, valueIssue)}
                </p>
              )}
            </div>
            <button
              type="button"
              className={styles.remove}
              aria-label={t('wl.home.headers.remove', { index: position })}
              onClick={() => removeRow(row.key)}
            >
              {t('wl.home.headers.removeButton')}
            </button>
          </div>
        );
      })}
      <div className={styles.footer}>
        <button ref={addRef} type="button" className={styles.add} disabled={full} onClick={addRow}>
          {t('wl.home.headers.add')}
        </button>
        <p id={hintId} className={styles.hint}>
          {full
            ? t('wl.home.headers.limit', { max: MAX_RESPONSE_HEADERS })
            : t('wl.home.headers.count', { used: rows.length, max: MAX_RESPONSE_HEADERS })}
        </p>
      </div>
    </fieldset>
  );
}
