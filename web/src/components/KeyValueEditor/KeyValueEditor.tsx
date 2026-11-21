import {
  useEffect,
  useId,
  useImperativeHandle,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
} from 'react';
import { useTranslation } from 'react-i18next';
import styles from './KeyValueEditor.module.css';
import {
  createKeyValueRow,
  findLengthFailures,
  moveRow,
  removeRow,
  updateRow,
  type KeyValueRow,
  type KeyValueRowError,
} from './rows';

export interface ValueFieldRenderProps {
  row: KeyValueRow;
  label: string;
  readOnly: boolean;
  invalid: boolean;
  describedBy: string | undefined;
  onChange: (value: string) => void;
}

export interface KeyValueEditorHandle {
  focusRow: (rowId: string, field?: 'name' | 'value') => void;
}

export interface KeyValueEditorProps {
  label: string;
  rows: readonly KeyValueRow[];
  onChange: (rows: KeyValueRow[]) => void;
  errors?: readonly KeyValueRowError[];
  maxRows?: number;
  maxNameLength?: number;
  maxValueLength?: number;
  nameLabel?: string;
  valueLabel?: string;
  addLabel?: string;
  readOnly?: boolean;
  reorderable?: boolean;
  nameSuggestions?: readonly string[];
  renderValue?: (props: ValueFieldRenderProps) => ReactNode;
  ref?: Ref<KeyValueEditorHandle>;
}

interface PendingFocus {
  rowId: string | null;
  field: 'name' | 'value';
}

const NO_ERRORS: readonly KeyValueRowError[] = [];

function sameRowErrors(errors: readonly KeyValueRowError[], rowId: string): KeyValueRowError[] {
  return errors.filter((error) => error.rowId === rowId);
}

export function KeyValueEditor({
  label,
  rows,
  onChange,
  errors = NO_ERRORS,
  maxRows,
  maxNameLength,
  maxValueLength,
  nameLabel,
  valueLabel,
  addLabel,
  readOnly = false,
  reorderable = true,
  nameSuggestions,
  renderValue,
  ref,
}: KeyValueEditorProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const rowElements = useRef(new Map<string, HTMLFieldSetElement>());
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const pendingFocus = useRef<PendingFocus | null>(null);
  const pendingRefocus = useRef<HTMLElement | null>(null);
  const [announcement, setAnnouncement] = useState('');

  const resolvedNameLabel = nameLabel ?? t('wl.keyvalue.name');
  const resolvedValueLabel = valueLabel ?? t('wl.keyvalue.value');
  const atLimit = maxRows !== undefined && rows.length >= maxRows;
  const lengthErrors: KeyValueRowError[] = findLengthFailures(rows, { maxNameLength, maxValueLength }).map(
    (failure) => ({
      rowId: failure.rowId,
      field: failure.field,
      message:
        failure.field === 'name'
          ? t('wl.keyvalue.nameTooLong', { max: failure.max })
          : t('wl.keyvalue.valueTooLong', { max: failure.max }),
    }),
  );
  const allErrors = [...errors, ...lengthErrors];
  const suggestionsId = `${baseId}-names`;

  function focusField(rowId: string, field: 'name' | 'value'): void {
    const element = rowElements.current.get(rowId);
    if (element === undefined) {
      return;
    }
    const target =
      element.querySelector<HTMLElement>(`[data-field="${field}"]`) ??
      element.querySelector<HTMLElement>('[data-field="name"]');
    target?.focus();
  }

  useImperativeHandle(ref, () => ({ focusRow: (rowId, field = 'name') => focusField(rowId, field) }));

  useEffect(() => {
    const pending = pendingFocus.current;
    if (pending !== null) {
      pendingFocus.current = null;
      if (pending.rowId === null) {
        addButtonRef.current?.focus();
      } else {
        focusField(pending.rowId, pending.field);
      }
    }
    const refocus = pendingRefocus.current;
    if (refocus !== null) {
      pendingRefocus.current = null;
      if (document.activeElement !== refocus) {
        refocus.focus();
      }
    }
  });

  function add(): void {
    if (atLimit) {
      return;
    }
    const row = createKeyValueRow();
    pendingFocus.current = { rowId: row.id, field: 'name' };
    setAnnouncement(t('wl.keyvalue.added', { index: rows.length + 1 }));
    onChange([...rows, row]);
  }

  function remove(index: number): void {
    const neighbour = rows[index + 1] ?? rows[index - 1];
    pendingFocus.current = { rowId: neighbour === undefined ? null : neighbour.id, field: 'name' };
    setAnnouncement(t('wl.keyvalue.removed', { index: index + 1 }));
    onChange(removeRow(rows, index));
  }

  function move(index: number, delta: number): void {
    const target = index + delta;
    if (target < 0 || target >= rows.length) {
      return;
    }
    const active = document.activeElement;
    pendingRefocus.current = active instanceof HTMLElement ? active : null;
    setAnnouncement(t('wl.keyvalue.moved', { position: target + 1, total: rows.length }));
    onChange(moveRow(rows, index, delta));
  }

  function handleKeyDown(event: KeyboardEvent<HTMLFieldSetElement>, index: number): void {
    if (readOnly || !reorderable || event.defaultPrevented || !event.altKey || !event.shiftKey) {
      return;
    }
    if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
      event.preventDefault();
      move(index, event.key === 'ArrowUp' ? -1 : 1);
    }
  }

  function renderField(row: KeyValueRow, index: number): ReactNode {
    const rowErrors = sameRowErrors(allErrors, row.id);
    const valueErrors = rowErrors.filter((error) => error.field === 'value');
    const nameErrors = rowErrors.filter((error) => error.field === 'name' || error.field === 'row');
    const nameDescribed = nameErrors.map((_, position) => `${baseId}-${row.id}-name-${position}`).join(' ');
    const valueDescribed = valueErrors.map((_, position) => `${baseId}-${row.id}-value-${position}`).join(' ');
    const rowLabel = t('wl.keyvalue.rowLabel', { index: index + 1 });
    const setValue = (value: string): void => {
      onChange(updateRow(rows, index, { value }));
    };
    return (
      <li key={row.id} className={styles.row}>
        <fieldset
          className={styles.fieldset}
          data-invalid={rowErrors.length > 0}
          ref={(element) => {
            if (element === null) {
              rowElements.current.delete(row.id);
            } else {
              rowElements.current.set(row.id, element);
            }
          }}
          onKeyDown={(event) => {
            handleKeyDown(event, index);
          }}
        >
          <legend className={styles.visuallyHidden}>{rowLabel}</legend>
          <div className={styles.fields}>
            <input
              type="text"
              className={styles.input}
              data-field="name"
              aria-label={`${rowLabel}: ${resolvedNameLabel}`}
              aria-invalid={nameErrors.length > 0}
              aria-describedby={nameDescribed === '' ? undefined : nameDescribed}
              list={nameSuggestions === undefined ? undefined : suggestionsId}
              autoComplete="off"
              spellCheck={false}
              readOnly={readOnly}
              value={row.name}
              onChange={(event) => {
                onChange(updateRow(rows, index, { name: event.target.value }));
              }}
            />
            {renderValue === undefined ? (
              <input
                type="text"
                className={styles.input}
                data-field="value"
                aria-label={`${rowLabel}: ${resolvedValueLabel}`}
                aria-invalid={valueErrors.length > 0}
                aria-describedby={valueDescribed === '' ? undefined : valueDescribed}
                autoComplete="off"
                spellCheck={false}
                readOnly={readOnly}
                value={row.value}
                onChange={(event) => {
                  setValue(event.target.value);
                }}
              />
            ) : (
              renderValue({
                row,
                label: `${rowLabel}: ${resolvedValueLabel}`,
                readOnly,
                invalid: valueErrors.length > 0,
                describedBy: valueDescribed === '' ? undefined : valueDescribed,
                onChange: setValue,
              })
            )}
          </div>
          {readOnly ? null : (
            <div className={styles.actions}>
              {reorderable ? (
                <>
                  <button
                    type="button"
                    className={styles.action}
                    aria-label={t('wl.keyvalue.moveUp', { index: index + 1 })}
                    aria-disabled={index === 0}
                    onClick={() => {
                      move(index, -1);
                    }}
                  >
                    <span aria-hidden="true">↑</span>
                  </button>
                  <button
                    type="button"
                    className={styles.action}
                    aria-label={t('wl.keyvalue.moveDown', { index: index + 1 })}
                    aria-disabled={index === rows.length - 1}
                    onClick={() => {
                      move(index, 1);
                    }}
                  >
                    <span aria-hidden="true">↓</span>
                  </button>
                </>
              ) : null}
              <button
                type="button"
                className={styles.action}
                aria-label={t('wl.keyvalue.remove', { index: index + 1 })}
                onClick={() => {
                  remove(index);
                }}
              >
                <span aria-hidden="true">×</span>
              </button>
            </div>
          )}
          {nameErrors.length > 0 || valueErrors.length > 0 ? (
            <div className={styles.errors}>
              {nameErrors.map((error, position) => (
                <p key={`name-${position}`} id={`${baseId}-${row.id}-name-${position}`} className={styles.error}>
                  {error.message}
                </p>
              ))}
              {valueErrors.map((error, position) => (
                <p key={`value-${position}`} id={`${baseId}-${row.id}-value-${position}`} className={styles.error}>
                  {error.message}
                </p>
              ))}
            </div>
          ) : null}
        </fieldset>
      </li>
    );
  }

  return (
    <div className={styles.editor}>
      <p className={styles.label} id={`${baseId}-label`}>
        {label}
      </p>
      {rows.length === 0 ? (
        <p className={styles.empty}>{t('wl.keyvalue.empty')}</p>
      ) : (
        <ul className={styles.rows} aria-labelledby={`${baseId}-label`}>
          {rows.map((row, index) => renderField(row, index))}
        </ul>
      )}
      {nameSuggestions === undefined ? null : (
        <datalist id={suggestionsId}>
          {nameSuggestions.map((suggestion) => (
            <option key={suggestion} value={suggestion} />
          ))}
        </datalist>
      )}
      {readOnly ? null : (
        <div className={styles.footer}>
          <button type="button" className={styles.add} ref={addButtonRef} disabled={atLimit} onClick={add}>
            {addLabel ?? t('wl.keyvalue.add')}
          </button>
          {maxRows === undefined ? null : (
            <span className={styles.count} data-full={atLimit}>
              {atLimit ? `${t('wl.keyvalue.limitReached')}. ` : ''}
              {t('wl.keyvalue.count', { used: rows.length, max: maxRows })}
            </span>
          )}
          {reorderable && rows.length > 1 ? <span className={styles.count}>{t('wl.keyvalue.moveHint')}</span> : null}
        </div>
      )}
      <div role="status" className={styles.visuallyHidden}>
        {announcement}
      </div>
    </div>
  );
}
