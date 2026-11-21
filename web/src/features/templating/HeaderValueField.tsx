import { useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { CodeEditor } from '../../components/CodeEditor/CodeEditor';
import { VariablePicker } from '../../components/VariablePicker/VariablePicker';
import type { CodeEditorHandle } from '../editor/types';
import type { HeaderValueRenderProps } from '../endpoints/HeadersEditor';
import { NO_PROBLEMS } from './diagnostics';
import styles from './HeaderValueField.module.css';
import type { TemplatingModel } from './useTemplating';

export interface HeaderValueFieldProps {
  field: HeaderValueRenderProps;
  model: TemplatingModel;
  readOnly: boolean;
  onSubmit: () => void;
}

export function HeaderValueField({ field, model, readOnly, onSubmit }: HeaderValueFieldProps) {
  const { t } = useTranslation();
  const handleRef = useRef<CodeEditorHandle | null>(null);
  const { index } = field;
  const { registerHeader } = model;
  const setHandle = useCallback(
    (handle: CodeEditorHandle | null) => {
      handleRef.current = handle;
      registerHeader(index, handle);
    },
    [index, registerHeader],
  );

  return (
    <div className={styles.field}>
      <div className={styles.editor}>
        <CodeEditor
          ref={setHandle}
          label={field.label}
          labelHidden
          language="template"
          singleLine
          readOnly={readOnly}
          delimiters={model.delimiters}
          value={field.row.value}
          onChange={field.onChange}
          completions={model.completions}
          diagnostics={model.grouped.headers.get(index) ?? NO_PROBLEMS}
          showDiagnostics={false}
          onSubmit={onSubmit}
        />
      </div>
      {readOnly ? null : (
        <VariablePicker
          compact
          catalogue={model.entries}
          loading={model.catalogueLoading}
          sampleCapture={model.captureLabel}
          triggerLabel={t('wl.templating.header.insert', { index: index + 1 })}
          onInsert={(entry) => {
            handleRef.current?.insertAtCursor(entry.insertText);
          }}
          onCloseFocus={() => {
            handleRef.current?.focus();
          }}
        />
      )}
    </div>
  );
}
