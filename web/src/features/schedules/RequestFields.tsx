import { useId, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { CodeEditor } from '../../components/CodeEditor/CodeEditor';
import type { CodeEditorHandle } from '../editor/types';
import { KeyValueEditor } from '../../components/KeyValueEditor/KeyValueEditor';
import type { KeyValueRowError } from '../../components/KeyValueEditor/rows';
import { VariablePicker } from '../../components/VariablePicker/VariablePicker';
import { variableCompletionSource, type VariableEntry } from '../../components/VariablePicker/variables';
import type { IssueText } from './issueText';
import { headerPath, type DraftIssue, type IssueMap, type ScheduleDraft } from './scheduleDraft';
import styles from './RequestFields.module.css';

export type RequestPatch = Partial<Pick<ScheduleDraft, 'method' | 'url' | 'headers' | 'body'>>;

export interface RequestFieldsProps {
  draft: Pick<ScheduleDraft, 'method' | 'url' | 'headers' | 'body'>;
  issues: IssueMap;
  issueText: IssueText;
  readOnly: boolean;
  onChange: (patch: RequestPatch) => void;
}

const METHOD_SUGGESTIONS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'] as const;
const TEMPLATE_DELIMITERS = { open: '{{', close: '}}' };

function useTemplateCatalogue(): readonly VariableEntry[] {
  const { t } = useTranslation();
  return useMemo(
    () => [
      {
        path: 'now',
        type: 'timestamp',
        insertText: '{{ now }}',
        group: 'time',
        description: t('wl.schedules.variable.now'),
      },
      {
        path: 'secret.NAME',
        type: 'secret',
        insertText: '{{ secret.NAME }}',
        group: 'helpers',
        description: t('wl.schedules.variable.secret'),
      },
    ],
    [t],
  );
}

function headerErrors(draft: RequestFieldsProps['draft'], issues: IssueMap, issueText: IssueText): KeyValueRowError[] {
  const errors: KeyValueRowError[] = [];
  draft.headers.forEach((row, index) => {
    for (const part of ['name', 'value'] as const) {
      const found: DraftIssue | undefined = issues.get(headerPath(index, part));
      if (found !== undefined) {
        errors.push({ rowId: row.id, field: part, message: issueText(found) });
      }
    }
  });
  return errors;
}

export function RequestFields({ draft, issues, issueText, readOnly, onChange }: RequestFieldsProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const bodyRef = useRef<CodeEditorHandle>(null);
  const methodId = `${baseId}-method`;
  const methodListId = `${baseId}-methods`;
  const urlId = `${baseId}-url`;
  const methodIssue = issues.get('request.method');
  const urlIssue = issues.get('request.url');
  const bodyIssue = issues.get('request.body');
  const catalogue = useTemplateCatalogue();
  const completions = useMemo(() => variableCompletionSource(catalogue), [catalogue]);
  const errors = useMemo(() => headerErrors(draft, issues, issueText), [draft, issues, issueText]);

  return (
    <fieldset className={styles.fieldset} disabled={readOnly}>
      <legend className={styles.legend}>{t('wl.schedules.editor.requestLegend')}</legend>
      <div className={styles.line}>
        <div className={styles.method}>
          <label htmlFor={methodId} className={styles.label}>
            {t('wl.schedules.editor.method')}
          </label>
          <input
            id={methodId}
            list={methodListId}
            type="text"
            className={styles.input}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={methodIssue === undefined ? undefined : true}
            aria-describedby={methodIssue === undefined ? undefined : `${methodId}-error`}
            value={draft.method}
            onChange={(event) => {
              onChange({ method: event.target.value });
            }}
          />
          <datalist id={methodListId}>
            {METHOD_SUGGESTIONS.map((method) => (
              <option key={method} value={method} />
            ))}
          </datalist>
          {methodIssue === undefined ? null : (
            <p id={`${methodId}-error`} className={styles.error}>
              {issueText(methodIssue)}
            </p>
          )}
        </div>
        <div className={styles.url}>
          <div className={styles.labelRow}>
            <label htmlFor={urlId} className={styles.label}>
              {t('wl.schedules.editor.url')}
            </label>
            <VariablePicker
              catalogue={catalogue}
              compact
              disabled={readOnly}
              onInsert={(entry) => {
                onChange({ url: `${draft.url}${entry.insertText}` });
              }}
            />
          </div>
          <input
            id={urlId}
            type="text"
            className={styles.input}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={urlIssue === undefined ? undefined : true}
            aria-describedby={urlIssue === undefined ? undefined : `${urlId}-error`}
            value={draft.url}
            onChange={(event) => {
              onChange({ url: event.target.value });
            }}
          />
          {urlIssue === undefined ? null : (
            <p id={`${urlId}-error`} className={styles.error}>
              {issueText(urlIssue)}
            </p>
          )}
        </div>
      </div>
      <KeyValueEditor
        label={t('wl.schedules.editor.headers')}
        rows={draft.headers}
        errors={errors}
        nameLabel={t('wl.schedules.editor.headerName')}
        valueLabel={t('wl.schedules.editor.headerValue')}
        addLabel={t('wl.schedules.editor.addHeader')}
        readOnly={readOnly}
        onChange={(headers) => {
          onChange({ headers });
        }}
      />
      <div className={styles.bodyBlock}>
        <div className={styles.pickerRow}>
          <VariablePicker
            catalogue={catalogue}
            compact
            disabled={readOnly}
            onInsert={(entry) => {
              bodyRef.current?.insertAtCursor(entry.insertText);
            }}
          />
        </div>
        <CodeEditor
          ref={bodyRef}
          label={t('wl.schedules.editor.body')}
          language="template"
          delimiters={TEMPLATE_DELIMITERS}
          completions={completions}
          value={draft.body}
          readOnly={readOnly}
          rows={6}
          onChange={(body) => {
            onChange({ body });
          }}
        />
        {bodyIssue === undefined ? null : <p className={styles.error}>{issueText(bodyIssue)}</p>}
      </div>
    </fieldset>
  );
}
