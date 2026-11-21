import { useTranslation } from 'react-i18next';
import { CodeEditor } from '../../components/CodeEditor/CodeEditor';
import { VariablePicker } from '../../components/VariablePicker/VariablePicker';
import { HeadersEditor } from '../endpoints/HeadersEditor';
import type { IssueMap } from '../endpoints/responseDraft';
import type { ResponseSettingsDraft } from '../workspace/response/responseSettings';
import { NO_PROBLEMS } from './diagnostics';
import { HeaderValueField } from './HeaderValueField';
import { languageOf } from './language';
import styles from './TemplateEditor.module.css';
import type { TemplatingModel } from './useTemplating';

export interface TemplateEditorProps {
  draft: ResponseSettingsDraft;
  onDraftChange: (draft: ResponseSettingsDraft) => void;
  model: TemplatingModel;
  issues: IssueMap;
  bodyMessage: string | null;
  headersMessage: string | null;
  readOnly: boolean;
  maxBodyBytes: number;
  onSubmit: () => void;
}

export function TemplateEditor({
  draft,
  onDraftChange,
  model,
  issues,
  bodyMessage,
  headersMessage,
  readOnly,
  maxBodyBytes,
  onSubmit,
}: TemplateEditorProps) {
  const { t } = useTranslation();
  const { enabled, setBodyHandle } = model;
  const bodyLabel = enabled ? t('wl.templating.body.template') : t('wl.home.field.body.label');

  return (
    <div className={styles.editor}>
      <section className={styles.block}>
        <HeadersEditor
          rows={draft.headers}
          issues={issues}
          onChange={(headers) => {
            onDraftChange({ ...draft, headers });
          }}
          renderValue={
            enabled
              ? (field) => <HeaderValueField field={field} model={model} readOnly={readOnly} onSubmit={onSubmit} />
              : undefined
          }
        />
        {headersMessage === null ? null : <p className={styles.error}>{headersMessage}</p>}
      </section>
      <section className={styles.block}>
        {enabled && !readOnly ? (
          <div className={styles.toolbar}>
            <VariablePicker
              catalogue={model.entries}
              loading={model.catalogueLoading}
              sampleCapture={model.captureLabel}
              onInsert={(entry) => {
                model.insertIntoBody(entry.insertText);
              }}
              onCloseFocus={() => {
                model.focusBody();
              }}
            />
          </div>
        ) : null}
        <CodeEditor
          ref={setBodyHandle}
          label={bodyLabel}
          language={languageOf(draft.contentType)}
          readOnly={readOnly}
          delimiters={enabled ? model.delimiters : undefined}
          completions={enabled ? model.completions : undefined}
          diagnostics={enabled ? model.grouped.body : NO_PROBLEMS}
          showDiagnostics={false}
          maxBytes={maxBodyBytes}
          rows={10}
          value={draft.body}
          onChange={(body) => {
            onDraftChange({ ...draft, body });
          }}
          onSubmit={enabled ? onSubmit : undefined}
        />
        {bodyMessage === null ? null : <p className={styles.error}>{bodyMessage}</p>}
      </section>
    </div>
  );
}
