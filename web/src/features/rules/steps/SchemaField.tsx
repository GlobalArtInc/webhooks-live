import { useMemo, useState, type ChangeEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { CodeEditor } from '../../../components/CodeEditor/CodeEditor';
import { KeyValueEditor } from '../../../components/KeyValueEditor/KeyValueEditor';
import type { KeyValueRow, KeyValueRowError } from '../../../components/KeyValueEditor/rows';
import { variableCompletionSource } from '../../../components/VariablePicker/variables';
import { ScriptStepEditor } from '../script/ScriptStepEditor';
import { issueMessage } from './issueMessage';
import { formatDuration, pairIssue, pairsFromRows, rowsFromPairs, samePairs } from './model';
import styles from './SchemaField.module.css';
import { TemplateControl } from './TemplateControl';
import type { FieldValue, KeyValuePair, SelectOption, StepEditorContext, StepFieldSchema } from './types';

export interface SchemaFieldProps {
  field: StepFieldSchema;
  value: FieldValue;
  onChange: (value: FieldValue) => void;
  idPrefix: string;
  context: StepEditorContext;
  error?: string | null;
  readOnly?: boolean;
}

interface ChoiceList {
  options: SelectOption[];
  missing: string | null;
}

const EMPTY_PAIRS: KeyValuePair[] = [];
const CODE_ROWS = 8;
const BODY_ROWS = 6;

function textOf(value: FieldValue): string {
  return typeof value === 'string' ? value : '';
}

function choices(base: readonly SelectOption[], current: string): ChoiceList {
  const known = base.some((option) => option.value === current);
  return { options: [...base], missing: current === '' || known ? null : current };
}

export function SchemaField({
  field,
  value,
  onChange,
  idPrefix,
  context,
  error = null,
  readOnly = false,
}: SchemaFieldProps) {
  const { t, i18n } = useTranslation();
  const id = `${idPrefix}-${field.name}`;
  const helpId = `${id}-help`;
  const errorId = `${id}-error`;
  const completions = useMemo(() => variableCompletionSource(context.variables), [context.variables]);
  const [rows, setRows] = useState<KeyValueRow[]>(() =>
    field.kind === 'key_value' && Array.isArray(value) ? rowsFromPairs(value) : [],
  );

  const pairs = field.kind === 'key_value' && Array.isArray(value) ? value : EMPTY_PAIRS;
  if (field.kind === 'key_value' && !samePairs(rows, pairs)) {
    setRows(rowsFromPairs(pairs));
  }

  const hasHelp = field.help !== undefined && field.help !== '';
  const describedBy = [hasHelp ? helpId : null, error === null ? null : errorId]
    .filter((item): item is string => item !== null)
    .join(' ');
  const describedByProp = describedBy === '' ? undefined : describedBy;
  const invalid = error !== null;

  function emitText(text: string): void {
    onChange(text === '' && field.nullable === true ? null : text);
  }

  function labelNode(): ReactNode {
    return (
      <label htmlFor={id} className={styles.label}>
        {field.label}
        {field.required === true ? <span className={styles.required}>{t('wl.steps.field.required')}</span> : null}
      </label>
    );
  }

  function selectNode(base: readonly SelectOption[], emptyLabel: string, missingLabel: (name: string) => string) {
    const current = textOf(value);
    const list = choices(base, current);
    return (
      <select
        id={id}
        className={styles.control}
        value={current}
        disabled={readOnly}
        aria-invalid={invalid}
        aria-required={field.required === true}
        aria-describedby={describedByProp}
        onChange={(event: ChangeEvent<HTMLSelectElement>) => {
          emitText(event.target.value);
        }}
      >
        {field.required === true && current !== '' ? null : <option value="">{emptyLabel}</option>}
        {list.missing === null ? null : <option value={list.missing}>{missingLabel(list.missing)}</option>}
        {list.options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    );
  }

  function body(): ReactNode {
    switch (field.kind) {
      case 'text':
      case 'json_path':
      case 'header_name':
        return (
          <>
            {labelNode()}
            <input
              id={id}
              type="text"
              className={styles.control}
              data-mono={field.kind !== 'text'}
              value={textOf(value)}
              placeholder={field.placeholder}
              readOnly={readOnly}
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              aria-invalid={invalid}
              aria-required={field.required === true}
              aria-describedby={describedByProp}
              onChange={(event) => {
                emitText(event.target.value);
              }}
            />
          </>
        );
      case 'number':
      case 'duration': {
        const amount = typeof value === 'number' ? value : null;
        return (
          <>
            {labelNode()}
            <div className={styles.numberRow}>
              <input
                id={id}
                type="number"
                inputMode="numeric"
                className={styles.control}
                data-narrow="true"
                value={amount === null ? '' : amount}
                min={field.min}
                max={field.max}
                step={1}
                readOnly={readOnly}
                aria-invalid={invalid}
                aria-required={field.required === true}
                aria-describedby={describedByProp}
                onChange={(event) => {
                  const raw = event.target.value;
                  const parsed = Number(raw);
                  onChange(raw === '' || Number.isNaN(parsed) ? null : parsed);
                }}
              />
              {field.kind === 'duration' ? (
                <span className={styles.unit}>
                  {t('wl.steps.field.milliseconds')}
                  {amount !== null && amount >= 1000
                    ? ` (${t('wl.steps.field.durationAbout', { text: formatDuration(amount, i18n.language) })})`
                    : ''}
                </span>
              ) : null}
            </div>
          </>
        );
      }
      case 'boolean':
        return (
          <label className={styles.check}>
            <input
              id={id}
              type="checkbox"
              checked={value === true}
              disabled={readOnly}
              aria-describedby={describedByProp}
              onChange={(event) => {
                onChange(event.target.checked);
              }}
            />
            <span>{field.label}</span>
          </label>
        );
      case 'select':
        return (
          <>
            {labelNode()}
            {selectNode(field.options ?? [], t('wl.steps.field.choose'), (name) =>
              t('wl.steps.field.unavailable', { value: name }),
            )}
          </>
        );
      case 'channel':
        return (
          <>
            {labelNode()}
            {selectNode(
              context.channels.map((channel) => ({ value: channel.id, label: channel.label })),
              t('wl.steps.field.chooseChannel'),
              () => t('wl.steps.field.unknownChannel'),
            )}
          </>
        );
      case 'secret':
        return (
          <>
            {labelNode()}
            {selectNode(
              context.secrets.map((name) => ({ value: name, label: name })),
              t('wl.steps.field.chooseSecret'),
              (name) => t('wl.steps.field.unknownSecret', { value: name }),
            )}
            <p className={styles.help}>
              {context.secrets.length === 0 ? t('wl.steps.field.noSecrets') : t('wl.steps.field.secretNote')}
            </p>
          </>
        );
      case 'template':
        return (
          <TemplateControl
            label={field.label}
            value={textOf(value)}
            onChange={emitText}
            variables={context.variables}
            completions={completions}
            singleLine={field.multiline !== true}
            readOnly={readOnly}
            maxBytes={field.maxBytes}
            rows={field.multiline === true ? BODY_ROWS : undefined}
          />
        );
      case 'code':
        if (field.language === 'script' && context.scriptEditor === true) {
          return (
            <ScriptStepEditor
              label={field.label}
              value={textOf(value)}
              onChange={emitText}
              readOnly={readOnly}
              rows={CODE_ROWS}
            />
          );
        }
        return (
          <CodeEditor
            label={field.label}
            language={field.language ?? 'text'}
            value={textOf(value)}
            onChange={emitText}
            readOnly={readOnly}
            maxBytes={field.maxBytes}
            rows={CODE_ROWS}
          />
        );
      case 'key_value': {
        const rowErrors: KeyValueRowError[] = [];
        rows.forEach((row) => {
          const issue = pairIssue(field, { name: row.name, value: row.value });
          if (issue !== null) {
            rowErrors.push({ rowId: row.id, field: 'name', message: issueMessage(t, issue) });
          }
        });
        return (
          <KeyValueEditor
            label={field.label}
            rows={rows}
            errors={rowErrors}
            maxRows={field.maxRows}
            nameLabel={field.nameKind === 'header_name' ? t('wl.steps.field.headerName') : undefined}
            readOnly={readOnly}
            onChange={(next) => {
              setRows(next);
              onChange(pairsFromRows(next));
            }}
            renderValue={
              field.valueKind === 'template'
                ? (props) => (
                    <TemplateControl
                      label={props.label}
                      labelHidden
                      value={props.row.value}
                      onChange={props.onChange}
                      variables={context.variables}
                      completions={completions}
                      singleLine
                      readOnly={props.readOnly}
                    />
                  )
                : undefined
            }
          />
        );
      }
    }
  }

  return (
    <div className={styles.field} data-schema-field={field.name} data-kind={field.kind} data-invalid={invalid}>
      {body()}
      {hasHelp ? (
        <p id={helpId} className={styles.help}>
          {field.help}
        </p>
      ) : null}
      {error === null ? null : (
        <p id={errorId} className={styles.error}>
          {error}
        </p>
      )}
    </div>
  );
}
