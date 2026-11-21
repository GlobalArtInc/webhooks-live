import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { isCidr } from '../../features/rules/conditions/cidr';
import type { ResolvedArgKind } from '../../features/rules/conditions/catalogue';
import type { EditorDiagnostic } from '../../features/editor/types';
import type { LeafDraft, SubjectEntry } from '../../features/rules/conditions/types';
import { ChipInput } from '../ChipInput/ChipInput';
import { CodeEditor } from '../CodeEditor/CodeEditor';
import styles from './ConditionRow.module.css';

export interface ValueFieldProps {
  kind: ResolvedArgKind;
  leaf: LeafDraft;
  subject: SubjectEntry | undefined;
  inputId: string;
  invalid: boolean;
  describedBy: string | undefined;
  serverMessage: string | undefined;
  readOnly: boolean;
  onText: (text: string) => void;
  onList: (list: string[]) => void;
}

export function ValueField({
  kind,
  leaf,
  subject,
  inputId,
  invalid,
  describedBy,
  serverMessage,
  readOnly,
  onText,
  onList,
}: ValueFieldProps) {
  const { t } = useTranslation();
  const label = t('wl.conditions.row.value');
  const suggestions = subject?.suggestions;
  const suggestionsId = `${inputId}-suggestions`;
  const diagnostics = useMemo<EditorDiagnostic[]>(
    () =>
      serverMessage === undefined
        ? []
        : [
            {
              line: 1,
              column: 1,
              length: Math.max(1, leaf.argText.length),
              code: 'regex_invalid',
              message: serverMessage,
            },
          ],
    [serverMessage, leaf.argText],
  );
  const invalidChips = useMemo(
    () => (kind === 'cidr_list' ? leaf.argList.filter((entry) => !isCidr(entry)) : []),
    [kind, leaf.argList],
  );

  switch (kind) {
    case 'none':
      return null;
    case 'regex':
      return (
        <div data-part="arg" className={styles.editor}>
          <CodeEditor
            label={label}
            language="text"
            singleLine
            readOnly={readOnly}
            value={leaf.argText}
            onChange={onText}
            diagnostics={diagnostics}
            showDiagnostics={false}
          />
        </div>
      );
    case 'list':
    case 'cidr_list':
      return (
        <div data-part="arg" className={styles.chips}>
          <ChipInput
            label={label}
            values={leaf.argList}
            onChange={onList}
            readOnly={readOnly}
            suggestions={suggestions}
            invalidValues={invalidChips}
            hint={kind === 'cidr_list' ? t('wl.conditions.hint.cidr') : t('wl.conditions.hint.list')}
            validate={
              kind === 'cidr_list'
                ? (value) => (isCidr(value) ? null : t('wl.conditions.issue.argCidr', { detail: value }))
                : undefined
            }
          />
        </div>
      );
    default:
      return (
        <div className={styles.cell}>
          <label className={styles.label} htmlFor={inputId}>
            {label}
          </label>
          <input
            id={inputId}
            data-part="arg"
            className={styles.input}
            type="text"
            inputMode={kind === 'number' ? 'decimal' : undefined}
            autoComplete="off"
            spellCheck={false}
            readOnly={readOnly}
            list={suggestions === undefined ? undefined : suggestionsId}
            value={leaf.argText}
            aria-invalid={invalid}
            aria-describedby={describedBy}
            onChange={(event) => {
              onText(event.target.value);
            }}
          />
          {suggestions === undefined ? null : (
            <datalist id={suggestionsId}>
              {suggestions.map((suggestion) => (
                <option key={suggestion} value={suggestion} />
              ))}
            </datalist>
          )}
        </div>
      );
  }
}
