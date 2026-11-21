import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { EditorDiagnostic } from '../../features/editor/types';
import styles from './DiagnosticList.module.css';

export interface DiagnosticListProps {
  diagnostics: readonly EditorDiagnostic[];
  onGo: (diagnostic: EditorDiagnostic) => void;
  id?: string;
}

interface DiagnosticGroupProps {
  heading: string;
  tone: 'error' | 'warning';
  items: readonly EditorDiagnostic[];
  onGo: (diagnostic: EditorDiagnostic) => void;
}

function DiagnosticGroup({ heading, tone, items, onGo }: DiagnosticGroupProps) {
  const { t } = useTranslation();
  const headingId = useId();
  return (
    <section className={styles.group} data-tone={tone}>
      <p id={headingId} className={styles.heading}>
        {heading}
      </p>
      <ul className={styles.list} aria-labelledby={headingId}>
        {items.map((item, index) => (
          <li key={`${item.line}:${item.column}:${item.code}:${index}`} className={styles.item}>
            <span className={styles.position}>
              {t('wl.editor.errors.position', { line: item.line, column: item.column })}
            </span>
            <span className={styles.message}>{item.message}</span>
            <button
              type="button"
              className={styles.go}
              aria-label={t('wl.editor.errors.goTo', { line: item.line, column: item.column, message: item.message })}
              onClick={() => {
                onGo(item);
              }}
            >
              {t('wl.editor.errors.go')}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function DiagnosticList({ diagnostics, onGo, id }: DiagnosticListProps) {
  const { t } = useTranslation();
  const errors = diagnostics.filter((item) => (item.severity ?? 'error') === 'error');
  const warnings = diagnostics.filter((item) => item.severity === 'warning');
  if (errors.length === 0 && warnings.length === 0) {
    return null;
  }
  return (
    <div id={id} className={styles.diagnostics}>
      {errors.length > 0 ? (
        <DiagnosticGroup
          heading={t('wl.editor.errors.heading', { number: errors.length })}
          tone="error"
          items={errors}
          onGo={onGo}
        />
      ) : null}
      {warnings.length > 0 ? (
        <DiagnosticGroup
          heading={t('wl.editor.warnings.heading', { number: warnings.length })}
          tone="warning"
          items={warnings}
          onGo={onGo}
        />
      ) : null}
    </div>
  );
}
