import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import {
  findOperator,
  findSubject,
  operatorsFor,
  resolveArgKind,
  subjectAvailable,
  subjectsOf,
} from '../../features/rules/conditions/catalogue';
import type { LeafPatch } from '../../features/rules/conditions/model';
import type {
  ConditionCatalogue,
  LeafDraft,
  RowIssue,
  RowPart,
  TriggerKind,
} from '../../features/rules/conditions/types';
import { variablePathOf } from '../../features/rules/conditions/variablePath';
import { VariablePicker } from '../VariablePicker/VariablePicker';
import type { VariableEntry } from '../VariablePicker/variables';
import styles from './ConditionRow.module.css';
import { issueText, operatorLabel, parameterLabel, parameterPlaceholder, subjectLabel } from './labels';
import { ValueField } from './ValueField';

export interface ConditionRowProps {
  leaf: LeafDraft;
  number: number;
  catalogue: ConditionCatalogue;
  triggers: readonly TriggerKind[];
  variables: readonly VariableEntry[];
  issues: readonly RowIssue[];
  showIssues: boolean;
  readOnly: boolean;
  canNegate: boolean;
  onChange: (patch: LeafPatch) => void;
  onRemove: () => void;
}

function describedByOf(baseId: string, issues: readonly RowIssue[], part: RowPart): string | undefined {
  const ids = issues.flatMap((issue, index) => (issue.part === part ? [`${baseId}-issue-${index}`] : []));
  return ids.length === 0 ? undefined : ids.join(' ');
}

export function ConditionRow({
  leaf,
  number,
  catalogue,
  triggers,
  variables,
  issues,
  showIssues,
  readOnly,
  canNegate,
  onChange,
  onRemove,
}: ConditionRowProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const subject = leaf.subject === '' ? undefined : findSubject(catalogue, leaf.subject);
  const operator = findOperator(catalogue, leaf.op);
  const argKind = resolveArgKind(operator, subject);
  const visible = showIssues ? issues : issues.filter((issue) => issue.message !== undefined);
  const incomplete = issues.length > 0 && visible.length === 0;
  const subjects = subjectsOf(catalogue).filter(
    (entry) => entry.id === leaf.subject || subjectAvailable(entry, triggers),
  );
  const operators = operatorsFor(catalogue, subject);
  const operatorListed = operators.some((entry) => entry.id === leaf.op);
  const parameter = subject?.parameter ?? null;
  const invalidIn = (part: RowPart): boolean => visible.some((issue) => issue.part === part);
  const regexMessage = visible.find((issue) => issue.part === 'arg')?.message;

  function changeSubject(id: string): void {
    const next = findSubject(catalogue, id);
    const allowed = operatorsFor(catalogue, next);
    const op = allowed.some((entry) => entry.id === leaf.op) ? leaf.op : (allowed[0]?.id ?? leaf.op);
    const keepsParameter = next?.parameter?.kind === parameter?.kind;
    onChange({ subject: id, op, parameter: keepsParameter ? leaf.parameter : '' });
  }

  function changeOperator(id: string): void {
    const kind = resolveArgKind(findOperator(catalogue, id), subject);
    const patch: LeafPatch = { op: id };
    if ((kind === 'list' || kind === 'cidr_list') && leaf.argList.length === 0 && leaf.argText.trim() !== '') {
      patch.argList = [leaf.argText.trim()];
    }
    if (
      (kind === 'text' || kind === 'number' || kind === 'regex') &&
      leaf.argText === '' &&
      leaf.argList[0] !== undefined
    ) {
      patch.argText = leaf.argList[0];
    }
    onChange(patch);
  }

  function pickVariable(entry: VariableEntry): void {
    onChange({ parameter: variablePathOf(entry) });
  }

  return (
    <fieldset
      className={styles.row}
      data-row-id={leaf.id}
      data-state={visible.length > 0 ? 'invalid' : incomplete ? 'incomplete' : 'ok'}
      aria-describedby={describedByOf(baseId, visible, 'row')}
    >
      <legend className={styles.legend}>
        {t('wl.conditions.row.legend', { number })}
        {incomplete ? <span className={styles.incomplete}>{t('wl.conditions.row.incomplete')}</span> : null}
      </legend>
      <div className={styles.fields}>
        <div className={styles.cell}>
          <label className={styles.label} htmlFor={`${baseId}-subject`}>
            {t('wl.conditions.row.subject')}
          </label>
          <select
            id={`${baseId}-subject`}
            data-part="subject"
            className={styles.input}
            disabled={readOnly}
            value={leaf.subject}
            aria-invalid={invalidIn('subject')}
            aria-describedby={describedByOf(baseId, visible, 'subject')}
            onChange={(event) => {
              changeSubject(event.target.value);
            }}
          >
            <option value="">{t('wl.conditions.row.subjectPlaceholder')}</option>
            {subject === undefined && leaf.subject !== '' ? (
              <option value={leaf.subject} lang="en">
                {t('wl.conditions.row.unknownSubject', { id: leaf.subject })}
              </option>
            ) : null}
            {subjects.map((entry) => {
              const label = subjectLabel(t, entry);
              return (
                <option key={entry.id} value={entry.id} lang={label.fallback ? 'en' : undefined}>
                  {label.text}
                </option>
              );
            })}
          </select>
        </div>
        {parameter === null ? null : (
          <div className={styles.cell}>
            <label className={styles.label} htmlFor={`${baseId}-parameter`}>
              {parameterLabel(t, parameter.kind)}
            </label>
            <div className={styles.withAction}>
              <input
                id={`${baseId}-parameter`}
                data-part="parameter"
                className={styles.input}
                type="text"
                autoComplete="off"
                spellCheck={false}
                readOnly={readOnly}
                placeholder={parameterPlaceholder(t, parameter.kind)}
                value={leaf.parameter}
                aria-invalid={invalidIn('parameter')}
                aria-describedby={describedByOf(baseId, visible, 'parameter')}
                onChange={(event) => {
                  onChange({ parameter: event.target.value });
                }}
              />
              {parameter.kind === 'variable' && !readOnly ? (
                <VariablePicker catalogue={variables} compact onInsert={pickVariable} />
              ) : null}
            </div>
          </div>
        )}
        <div className={styles.cell}>
          <label className={styles.label} htmlFor={`${baseId}-operator`}>
            {t('wl.conditions.row.operator')}
          </label>
          <select
            id={`${baseId}-operator`}
            data-part="operator"
            className={styles.input}
            disabled={readOnly}
            value={leaf.op}
            aria-invalid={invalidIn('operator')}
            aria-describedby={describedByOf(baseId, visible, 'operator')}
            onChange={(event) => {
              changeOperator(event.target.value);
            }}
          >
            {operatorListed ? null : (
              <option value={leaf.op} lang="en">
                {operator === undefined
                  ? t('wl.conditions.row.unknownOperator', { id: leaf.op })
                  : operatorLabel(t, operator).text}
              </option>
            )}
            {operators.map((entry) => {
              const label = operatorLabel(t, entry);
              return (
                <option key={entry.id} value={entry.id} lang={label.fallback ? 'en' : undefined}>
                  {label.text}
                </option>
              );
            })}
          </select>
        </div>
        <ValueField
          kind={argKind}
          leaf={leaf}
          subject={subject}
          inputId={`${baseId}-arg`}
          invalid={invalidIn('arg')}
          describedBy={describedByOf(baseId, visible, 'arg')}
          serverMessage={regexMessage}
          readOnly={readOnly}
          onText={(argText) => {
            onChange({ argText });
          }}
          onList={(argList) => {
            onChange({ argList });
          }}
        />
      </div>
      {readOnly ? null : (
        <div className={styles.actions}>
          <label className={styles.negate}>
            <input
              type="checkbox"
              checked={leaf.negate}
              disabled={!leaf.negate && !canNegate}
              aria-label={t('wl.conditions.negateRow', { number })}
              onChange={(event) => {
                onChange({ negate: event.target.checked });
              }}
            />
            <span>{t('wl.conditions.negate')}</span>
          </label>
          <button
            type="button"
            className={styles.remove}
            aria-label={t('wl.conditions.removeRow', { number })}
            onClick={onRemove}
          >
            <span aria-hidden="true">×</span>
          </button>
        </div>
      )}
      {visible.length === 0 ? null : (
        <ul className={styles.errors}>
          {visible.map((issue, index) => (
            <li key={`${issue.part}-${issue.code}-${index}`} id={`${baseId}-issue-${index}`} className={styles.error}>
              {issueText(t, issue)}
            </li>
          ))}
        </ul>
      )}
    </fieldset>
  );
}
