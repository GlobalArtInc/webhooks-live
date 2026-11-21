import {
  useEffect,
  useEffectEvent,
  useId,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type Ref,
} from 'react';
import { useTranslation } from 'react-i18next';
import { DEFAULT_DELIMITERS, DEFAULT_EDITOR_ROWS, DIAGNOSTICS_SETTLE_MS } from '../../features/editor/constants';
import { byteLength, positionToOffset } from '../../features/editor/positions';
import type {
  CodeEditorHandle,
  CompletionSource,
  EditorDiagnostic,
  EditorLanguage,
  EngineConfig,
  EngineHandle,
  TemplateDelimiters,
} from '../../features/editor/types';
import { formatByteSize } from '../format/format';
import styles from './CodeEditor.module.css';
import { DiagnosticList } from './DiagnosticList';

export interface CodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  language: EditorLanguage;
  label: string;
  labelHidden?: boolean;
  diagnostics?: readonly EditorDiagnostic[];
  completions?: CompletionSource;
  delimiters?: TemplateDelimiters;
  scriptKeywords?: readonly string[];
  readOnly?: boolean;
  singleLine?: boolean;
  maxBytes?: number;
  rows?: number;
  onSubmit?: () => void;
  showDiagnostics?: boolean;
  ref?: Ref<CodeEditorHandle>;
}

type EditorStatus = 'loading' | 'ready' | 'failed';

type PendingAction = { kind: 'focus' } | { kind: 'insert'; text: string };

interface RowsStyle extends CSSProperties {
  '--editor-rows': number;
}

const NO_DIAGNOSTICS: readonly EditorDiagnostic[] = [];

function severityOf(diagnostic: EditorDiagnostic): 'error' | 'warning' {
  return diagnostic.severity ?? 'error';
}

export function CodeEditor({
  value,
  onChange,
  language,
  label,
  labelHidden = false,
  diagnostics = NO_DIAGNOSTICS,
  completions,
  delimiters,
  scriptKeywords,
  readOnly = false,
  singleLine = false,
  maxBytes,
  rows = DEFAULT_EDITOR_ROWS,
  onSubmit,
  showDiagnostics = true,
  ref,
}: CodeEditorProps) {
  const { t, i18n } = useTranslation();
  const labelId = useId();
  const hintId = useId();
  const sizeId = useId();
  const listId = useId();
  const hostRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const engineRef = useRef<EngineHandle | null>(null);
  const announcedRef = useRef('');
  const pendingRef = useRef<PendingAction[]>([]);
  const [status, setStatus] = useState<EditorStatus>('loading');
  const [tabEscape, setTabEscape] = useState(false);
  const [announcement, setAnnouncement] = useState('');

  const used = useMemo(() => byteLength(value), [value]);
  const over = maxBytes !== undefined && used > maxBytes;
  const errorCount = diagnostics.filter((item) => severityOf(item) === 'error').length;
  const warningCount = diagnostics.length - errorCount;
  const invalid = over || errorCount > 0;
  const activeDelimiters = delimiters ?? (language === 'template' ? DEFAULT_DELIMITERS : null);
  const delimiterOpen = activeDelimiters?.open;
  const delimiterClose = activeDelimiters?.close;

  const buildConfig = useEffectEvent((): EngineConfig => ({
    value,
    language,
    label,
    readOnly,
    singleLine,
    invalid,
    delimiters: activeDelimiters,
    scriptKeywords: scriptKeywords ?? null,
    diagnostics,
    completions: completions ?? null,
    onChange: onChange ?? null,
    onSubmit: onSubmit ?? null,
    onTabEscapeChange: setTabEscape,
  }));

  function goTo(line: number, column: number): void {
    const engine = engineRef.current;
    if (engine !== null) {
      engine.goTo(line, column);
      return;
    }
    const area = textareaRef.current;
    if (area !== null) {
      const offset = positionToOffset(value, line, column);
      area.focus();
      area.setSelectionRange(offset, offset);
    }
  }

  function perform(action: PendingAction): boolean {
    const engine = engineRef.current;
    if (engine !== null) {
      if (action.kind === 'focus') {
        engine.focus();
      } else {
        engine.insertAtCursor(action.text);
      }
      return true;
    }
    const area = textareaRef.current;
    if (area === null) {
      return false;
    }
    if (action.kind === 'focus') {
      area.focus();
    } else if (!readOnly) {
      area.setRangeText(action.text, area.selectionStart, area.selectionEnd, 'end');
      onChange?.(area.value);
      area.focus();
    }
    return true;
  }

  function submit(action: PendingAction): void {
    if (!perform(action)) {
      pendingRef.current.push(action);
    }
  }

  const drainPending = useEffectEvent((): void => {
    const queued = pendingRef.current;
    pendingRef.current = [];
    for (const action of queued) {
      perform(action);
    }
  });

  useImperativeHandle(ref, () => ({
    focus: () => {
      submit({ kind: 'focus' });
    },
    insertAtCursor: (text: string) => {
      submit({ kind: 'insert', text });
    },
    goTo,
  }));

  useEffect(() => {
    const host = hostRef.current;
    if (host === null) {
      return undefined;
    }
    let cancelled = false;
    import('../../features/editor/engine').then(
      (module) => {
        if (cancelled) {
          return;
        }
        engineRef.current = module.createEngine(host, buildConfig());
        drainPending();
        setStatus('ready');
      },
      () => {
        if (!cancelled) {
          setStatus('failed');
        }
      },
    );
    return () => {
      cancelled = true;
      engineRef.current?.destroy();
      engineRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (status === 'failed') {
      drainPending();
    }
  }, [status]);

  useEffect(() => {
    if (status === 'ready') {
      engineRef.current?.update(buildConfig());
    }
  }, [
    status,
    value,
    language,
    label,
    readOnly,
    singleLine,
    invalid,
    delimiterOpen,
    delimiterClose,
    scriptKeywords,
    diagnostics,
    completions,
    onChange,
    onSubmit,
  ]);

  useEffect(() => {
    const key = `${errorCount}:${warningCount}`;
    const timer = setTimeout(() => {
      if (key === announcedRef.current || (key === '0:0' && announcedRef.current === '')) {
        return;
      }
      announcedRef.current = key;
      const parts: string[] = [];
      if (errorCount > 0) {
        parts.push(t('wl.editor.announce.errors', { count: errorCount, label }));
      }
      if (warningCount > 0) {
        parts.push(t('wl.editor.announce.warnings', { count: warningCount, label }));
      }
      setAnnouncement(parts.length > 0 ? parts.join(' ') : t('wl.editor.announce.clear', { label }));
    }, DIAGNOSTICS_SETTLE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [errorCount, warningCount, label, t]);

  const locale = i18n.language;
  let sizeText = '';
  if (maxBytes !== undefined) {
    sizeText = over
      ? t('wl.editor.sizeOver', {
          used: formatByteSize(used, locale),
          max: formatByteSize(maxBytes, locale),
          over: formatByteSize(used - maxBytes, locale),
        })
      : t('wl.editor.sizeOf', { used: formatByteSize(used, locale), max: formatByteSize(maxBytes, locale) });
  }

  const showHint = !readOnly && !singleLine;
  const describedBy = [
    showHint ? hintId : null,
    maxBytes !== undefined ? sizeId : null,
    showDiagnostics && diagnostics.length > 0 ? listId : null,
  ]
    .filter((id): id is string => id !== null)
    .join(' ');
  const rowsStyle: RowsStyle = { '--editor-rows': rows };

  return (
    <div className={styles.field} data-status={status} data-read-only={readOnly} data-invalid={invalid}>
      <div className={styles.labelRow}>
        <span id={labelId} className={labelHidden ? styles.visuallyHidden : styles.label}>
          {label}
        </span>
        {readOnly ? <span className={styles.badge}>{t('wl.editor.readOnly')}</span> : null}
      </div>
      <div
        role="group"
        aria-labelledby={labelId}
        aria-describedby={describedBy === '' ? undefined : describedBy}
        className={styles.surface}
        style={rowsStyle}
        data-single-line={singleLine}
      >
        {status === 'failed' ? (
          <textarea
            ref={textareaRef}
            className={styles.fallback}
            aria-labelledby={labelId}
            aria-invalid={invalid}
            rows={singleLine ? 1 : rows}
            readOnly={readOnly}
            spellCheck={false}
            value={value}
            onChange={(event) => {
              onChange?.(event.target.value);
            }}
          />
        ) : (
          <div ref={hostRef} className={styles.host} />
        )}
        {status === 'loading' ? (
          <p className={styles.notice} role="status">
            {t('wl.editor.loading')}
          </p>
        ) : null}
      </div>
      {status === 'failed' ? (
        <p className={styles.failure} role="alert">
          {t('wl.editor.loadFailed')}
        </p>
      ) : null}
      {showHint || maxBytes !== undefined ? (
        <div className={styles.meta}>
          {showHint ? (
            <span id={hintId} className={styles.hint} data-active={tabEscape} aria-live="polite">
              {tabEscape ? t('wl.editor.tabHintActive') : t('wl.editor.tabHint')}
            </span>
          ) : null}
          {maxBytes !== undefined ? (
            <span id={sizeId} className={styles.size} data-over={over}>
              {sizeText}
            </span>
          ) : null}
        </div>
      ) : null}
      {showDiagnostics ? (
        <DiagnosticList id={listId} diagnostics={diagnostics} onGo={(item) => goTo(item.line, item.column)} />
      ) : null}
      <div role="status" className={styles.visuallyHidden}>
        {announcement}
      </div>
    </div>
  );
}
