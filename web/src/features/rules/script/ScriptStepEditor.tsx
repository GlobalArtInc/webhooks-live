import { useMemo, type Ref } from 'react';
import { ScriptEditor } from '../../../components/ScriptEditor/ScriptEditor';
import type { CodeEditorHandle, EditorDiagnostic } from '../../editor/types';
import { httpScriptTransport } from './transport';
import type { ScriptPhase, ScriptTransport } from './types';
import { useScriptDiagnostics } from './useScriptDiagnostics';
import { useScriptMeta } from './useScriptMeta';

export interface ScriptStepEditorProps {
  value: string;
  onChange?: (value: string) => void;
  label: string;
  labelHidden?: boolean;
  phase?: ScriptPhase;
  readOnly?: boolean;
  fullHeight?: boolean;
  rows?: number;
  offset?: string;
  extraDiagnostics?: readonly EditorDiagnostic[];
  transport?: ScriptTransport;
  onSubmit?: () => void;
  ref?: Ref<CodeEditorHandle>;
}

const NO_DIAGNOSTICS: readonly EditorDiagnostic[] = [];

export function ScriptStepEditor({
  extraDiagnostics = NO_DIAGNOSTICS,
  transport = httpScriptTransport,
  readOnly = false,
  ...rest
}: ScriptStepEditorProps) {
  const language = useScriptMeta(transport);
  const validation = useScriptDiagnostics(rest.value, {
    enabled: !readOnly,
    transport,
    maxBytes: language.meta?.limits.max_source_bytes,
  });
  const diagnostics = useMemo(
    () => (extraDiagnostics.length === 0 ? validation.diagnostics : [...validation.diagnostics, ...extraDiagnostics]),
    [validation.diagnostics, extraDiagnostics],
  );
  return (
    <ScriptEditor
      {...rest}
      readOnly={readOnly}
      languageMeta={language.meta}
      languageState={language.state}
      onRetryLanguage={language.retry}
      diagnostics={diagnostics}
      validationState={validation.state}
      onRetryValidation={validation.retry}
    />
  );
}
