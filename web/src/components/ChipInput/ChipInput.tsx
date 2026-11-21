import { useId, useRef, useState, type ClipboardEvent, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './ChipInput.module.css';
import { addChips, splitChips, type ChipFailure, type ChipNormalizer, type ChipValidator } from './chips';

export interface ChipInputProps {
  label: string;
  values: readonly string[];
  onChange: (values: string[]) => void;
  validate?: ChipValidator;
  normalize?: ChipNormalizer;
  invalidValues?: readonly string[];
  suggestions?: readonly string[];
  maxItems?: number;
  maxLength?: number;
  placeholder?: string;
  hint?: string;
  readOnly?: boolean;
  allowDuplicates?: boolean;
}

const NO_VALUES: readonly string[] = [];

export function ChipInput({
  label,
  values,
  onChange,
  validate,
  normalize,
  invalidValues = NO_VALUES,
  suggestions,
  maxItems,
  maxLength,
  placeholder,
  hint,
  readOnly = false,
  allowDuplicates = false,
}: ChipInputProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [draft, setDraft] = useState('');
  const [failure, setFailure] = useState<ChipFailure | null>(null);
  const [announcement, setAnnouncement] = useState('');

  const atLimit = maxItems !== undefined && values.length >= maxItems;
  const inputId = `${baseId}-input`;
  const hintId = `${baseId}-hint`;
  const errorId = `${baseId}-error`;
  const listId = `${baseId}-suggestions`;
  const describedBy = [failure === null ? null : errorId, hintId].filter((id): id is string => id !== null).join(' ');

  function failureText(problem: ChipFailure): string {
    switch (problem.kind) {
      case 'duplicate':
        return t('wl.chips.duplicate', { value: problem.value });
      case 'limit':
        return t('wl.chips.limit', { max: problem.detail });
      case 'tooLong':
        return t('wl.chips.tooLong', { max: problem.detail });
      case 'invalid':
        return problem.detail;
    }
  }

  function commit(text: string): void {
    const pieces = splitChips(text);
    if (pieces.length === 0) {
      setDraft('');
      setFailure(null);
      return;
    }
    const batch = addChips(values, pieces, { maxItems, maxLength, allowDuplicates, validate, normalize });
    if (batch.accepted.length > 0) {
      onChange([...values, ...batch.accepted]);
      setAnnouncement(t('wl.chips.added', { value: batch.accepted.join(', ') }));
    }
    setFailure(batch.failure);
    setDraft(batch.rest.join(', '));
  }

  function remove(index: number): void {
    const removed = values[index];
    onChange(values.filter((_, position) => position !== index));
    if (removed !== undefined) {
      setAnnouncement(t('wl.chips.removed', { value: removed }));
    }
    inputRef.current?.focus();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>): void {
    if (event.key === 'Enter' || event.key === ',') {
      if (draft.trim() !== '' || event.key === ',') {
        event.preventDefault();
        commit(draft);
      }
      return;
    }
    if (event.key === 'Backspace' && draft === '' && values.length > 0) {
      event.preventDefault();
      remove(values.length - 1);
    }
  }

  function handlePaste(event: ClipboardEvent<HTMLInputElement>): void {
    const text = event.clipboardData.getData('text');
    if (/[,\n\r]/.test(text)) {
      event.preventDefault();
      commit(`${draft}${text}`);
    }
  }

  return (
    <div className={styles.field}>
      {readOnly ? (
        <span className={styles.label}>{label}</span>
      ) : (
        <label className={styles.label} htmlFor={inputId}>
          {label}
        </label>
      )}
      <div
        className={styles.box}
        data-invalid={failure !== null}
        onClick={() => {
          inputRef.current?.focus();
        }}
      >
        {values.length > 0 ? (
          <ul className={styles.chips} aria-label={label}>
            {values.map((value, index) => {
              const invalid = invalidValues.includes(value);
              return (
                <li key={`${value}-${index}`} className={styles.chip} data-invalid={invalid}>
                  {invalid ? <span aria-hidden="true">! </span> : null}
                  <span className={styles.chipText}>{value}</span>
                  {invalid ? <span className={styles.visuallyHidden}>{t('wl.chips.invalidMark')}</span> : null}
                  {readOnly ? null : (
                    <button
                      type="button"
                      className={styles.remove}
                      aria-label={t('wl.chips.remove', { value })}
                      onClick={(event) => {
                        event.stopPropagation();
                        remove(index);
                      }}
                    >
                      <span aria-hidden="true">×</span>
                    </button>
                  )}
                </li>
              );
            })}
          </ul>
        ) : null}
        {readOnly ? null : (
          <input
            id={inputId}
            disabled={atLimit}
            ref={inputRef}
            type="text"
            className={styles.input}
            value={draft}
            placeholder={values.length === 0 ? (placeholder ?? t('wl.chips.placeholder')) : undefined}
            autoComplete="off"
            spellCheck={false}
            list={suggestions === undefined ? undefined : listId}
            aria-invalid={failure !== null}
            aria-describedby={describedBy}
            onChange={(event) => {
              setDraft(event.target.value);
              setFailure(null);
            }}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            onBlur={() => {
              if (draft.trim() !== '') {
                commit(draft);
              }
            }}
          />
        )}
        {readOnly && values.length === 0 ? <span className={styles.empty}>{t('wl.chips.empty')}</span> : null}
      </div>
      {suggestions === undefined ? null : (
        <datalist id={listId}>
          {suggestions.map((suggestion) => (
            <option key={suggestion} value={suggestion} />
          ))}
        </datalist>
      )}
      {readOnly ? null : (
        <p id={hintId} className={styles.hint}>
          {atLimit && maxItems !== undefined ? t('wl.chips.limit', { max: maxItems }) : (hint ?? t('wl.chips.hint'))}
        </p>
      )}
      {failure === null ? null : (
        <p id={errorId} className={styles.error} role="alert">
          {failureText(failure)}
        </p>
      )}
      <div role="status" className={styles.visuallyHidden}>
        {announcement}
      </div>
    </div>
  );
}
