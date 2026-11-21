import { useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { CodeEditor } from '../../../components/CodeEditor/CodeEditor';
import { VariablePicker, type VariablePickerProps } from '../../../components/VariablePicker/VariablePicker';
import type { VariableEntry } from '../../../components/VariablePicker/variables';
import type { CodeEditorHandle, CompletionSource } from '../../editor/types';
import styles from './TemplateControl.module.css';

export interface TemplateControlProps {
  label: string;
  labelHidden?: boolean;
  value: string;
  onChange: (value: string) => void;
  variables: readonly VariableEntry[];
  completions: CompletionSource;
  singleLine: boolean;
  readOnly: boolean;
  maxBytes?: number;
  rows?: number;
}

export function TemplateControl({
  label,
  labelHidden = false,
  value,
  onChange,
  variables,
  completions,
  singleLine,
  readOnly,
  maxBytes,
  rows,
}: TemplateControlProps) {
  const { t } = useTranslation();
  const editorRef = useRef<CodeEditorHandle>(null);

  const pickerProps: VariablePickerProps = {
    catalogue: variables,
    compact: true,
    disabled: readOnly,
    triggerLabel: t('wl.steps.field.insertInto', { label }),
    onInsert: (entry) => {
      editorRef.current?.insertAtCursor(entry.insertText);
    },
    onCloseFocus: () => {
      editorRef.current?.focus();
    },
  };

  const editor = (
    <CodeEditor
      ref={editorRef}
      label={label}
      labelHidden={labelHidden}
      language="template"
      value={value}
      onChange={onChange}
      completions={completions}
      readOnly={readOnly}
      singleLine={singleLine}
      maxBytes={maxBytes}
      rows={rows}
    />
  );

  if (singleLine) {
    return (
      <div className={styles.line}>
        <div className={styles.grow}>{editor}</div>
        <VariablePicker {...pickerProps} />
      </div>
    );
  }
  return (
    <div className={styles.block}>
      <div className={styles.toolbar}>
        <VariablePicker {...pickerProps} />
      </div>
      {editor}
    </div>
  );
}
