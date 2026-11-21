import { useEffect, useEffectEvent, useMemo, useRef, useState, type CSSProperties, type Ref } from 'react';
import { useTranslation } from 'react-i18next';
import type { CodeEditorHandle, EditorDiagnostic } from '../../features/editor/types';
import {
  formatCount,
  formatDurationMs,
  limitsFor,
  scriptCompletionSource,
} from '../../features/rules/script/catalogue';
import { DEFAULT_SCRIPT_ROWS, FULL_HEIGHT_ROWS } from '../../features/rules/script/constants';
import type {
  EnhancementConfig,
  Enhancements,
  ScriptLanguageMeta,
  ScriptLanguageState,
  ScriptPhase,
  ScriptValidationState,
} from '../../features/rules/script/types';
import { CodeEditor } from '../CodeEditor/CodeEditor';
import styles from './ScriptEditor.module.css';

export interface ScriptEditorProps {
  value: string;
  onChange?: (value: string) => void;
  label: string;
  labelHidden?: boolean;
  languageMeta: ScriptLanguageMeta | null;
  languageState?: ScriptLanguageState;
  onRetryLanguage?: () => void;
  diagnostics?: readonly EditorDiagnostic[];
  validationState?: ScriptValidationState;
  onRetryValidation?: () => void;
  phase?: ScriptPhase;
  readOnly?: boolean;
  fullHeight?: boolean;
  rows?: number;
  offset?: string;
  onSubmit?: () => void;
  ref?: Ref<CodeEditorHandle>;
}

interface OffsetStyle extends CSSProperties {
  '--script-editor-offset'?: string;
}

const NO_DIAGNOSTICS: readonly EditorDiagnostic[] = [];

export function ScriptEditor({
  value,
  onChange,
  label,
  labelHidden = false,
  languageMeta,
  languageState,
  onRetryLanguage,
  diagnostics = NO_DIAGNOSTICS,
  validationState = 'idle',
  onRetryValidation,
  phase = 'before',
  readOnly = false,
  fullHeight = false,
  rows = DEFAULT_SCRIPT_ROWS,
  offset,
  onSubmit,
  ref,
}: ScriptEditorProps) {
  const { t, i18n } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const enhancementsRef = useRef<Enhancements | null>(null);
  const [enhanceFailed, setEnhanceFailed] = useState(false);

  const language: ScriptLanguageState = languageState ?? (languageMeta === null ? 'loading' : 'ready');
  const keywords = languageMeta?.keywords;
  const completions = useMemo(
    () => (languageMeta === null ? undefined : scriptCompletionSource(languageMeta)),
    [languageMeta],
  );

  const readConfig = useEffectEvent((): EnhancementConfig => ({ meta: languageMeta, fullHeight }));

  useEffect(() => {
    const root = rootRef.current;
    if (root === null) {
      return undefined;
    }
    let cancelled = false;
    import('../../features/rules/script/enhance').then(
      (module) => {
        if (!cancelled) {
          enhancementsRef.current = module.attachEnhancements(root, readConfig());
        }
      },
      () => {
        if (!cancelled) {
          setEnhanceFailed(true);
        }
      },
    );
    return () => {
      cancelled = true;
      enhancementsRef.current?.destroy();
      enhancementsRef.current = null;
    };
  }, []);

  useEffect(() => {
    enhancementsRef.current?.update({ meta: languageMeta, fullHeight });
  }, [languageMeta, fullHeight]);

  const locale = i18n.language;
  let limitsText: string | null = null;
  if (languageMeta !== null && !readOnly) {
    const limits = limitsFor(languageMeta, phase);
    limitsText = t('wl.script.limits.line', {
      count: limits.max_operations,
      operations: formatCount(limits.max_operations, locale),
      time: formatDurationMs(limits.max_time_ms, locale),
    });
  }
  const reference = languageMeta?.reference_url ?? null;
  const rootStyle: OffsetStyle | undefined = offset === undefined ? undefined : { '--script-editor-offset': offset };

  return (
    <div ref={rootRef} className={styles.root} data-full-height={fullHeight} style={rootStyle}>
      <CodeEditor
        ref={ref}
        label={label}
        labelHidden={labelHidden}
        language="script"
        value={value}
        onChange={onChange}
        diagnostics={diagnostics}
        completions={completions}
        scriptKeywords={keywords}
        readOnly={readOnly}
        maxBytes={languageMeta?.limits.max_source_bytes}
        rows={fullHeight ? FULL_HEIGHT_ROWS : rows}
        onSubmit={onSubmit}
      />
      <div className={styles.notes}>
        {language === 'loading' ? (
          <p className={styles.note} role="status">
            {t('wl.script.languageLoading')}
          </p>
        ) : null}
        {language === 'failed' ? (
          <p className={styles.note} data-tone="warning" role="status">
            <span>{t('wl.script.languageFailed')}</span>
            {onRetryLanguage === undefined ? null : (
              <button type="button" className={styles.retry} onClick={onRetryLanguage}>
                {t('wl.script.retry')}
              </button>
            )}
          </p>
        ) : null}
        {enhanceFailed ? (
          <p className={styles.note} data-tone="warning" role="status">
            {t('wl.script.enhanceFailed')}
          </p>
        ) : null}
        {validationState === 'pending' ? (
          <p className={styles.note} aria-hidden="true">
            {t('wl.script.checking')}
          </p>
        ) : null}
        {validationState === 'failed' ? (
          <p className={styles.note} data-tone="warning" role="status">
            <span>{t('wl.script.checkFailed')}</span>
            {onRetryValidation === undefined ? null : (
              <button type="button" className={styles.retry} onClick={onRetryValidation}>
                {t('wl.script.retry')}
              </button>
            )}
          </p>
        ) : null}
        {limitsText === null ? null : (
          <p className={styles.note}>
            <span className={styles.phase}>
              {phase === 'before' ? t('wl.script.limits.before') : t('wl.script.limits.after')}
            </span>{' '}
            {limitsText}
          </p>
        )}
        {reference === null ? null : (
          <p className={styles.note}>
            <a className={styles.link} href={reference} target="_blank" rel="noopener noreferrer">
              {t('wl.script.reference')}
              <span className={styles.visuallyHidden}> {t('wl.script.referenceOpens')}</span>
            </a>
          </p>
        )}
      </div>
    </div>
  );
}
