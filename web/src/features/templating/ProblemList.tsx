import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { ProblemItem } from './diagnostics';
import styles from './ProblemList.module.css';

export interface ProblemListProps {
  problems: readonly ProblemItem[];
  onGo: (item: ProblemItem) => void;
  id?: string;
}

interface GroupProps {
  heading: string;
  tone: 'error' | 'warning';
  items: readonly ProblemItem[];
  onGo: (item: ProblemItem) => void;
}

function useFieldLabel(): (item: ProblemItem) => string {
  const { t } = useTranslation();
  return (item) => {
    const { target } = item;
    if (target.kind === 'body') {
      return t('wl.templating.field.body');
    }
    if (target.kind === 'header') {
      return t('wl.templating.field.header', { index: target.index + 1 });
    }
    if (target.field === 'content_type') {
      return t('wl.home.field.contentType.label');
    }
    if (target.field === 'status') {
      return t('wl.home.field.status.label');
    }
    return target.field;
  };
}

function ProblemGroup({ heading, tone, items, onGo }: GroupProps) {
  const { t } = useTranslation();
  const headingId = useId();
  const labelOf = useFieldLabel();
  return (
    <section className={styles.group} data-tone={tone}>
      <p id={headingId} className={styles.heading}>
        {heading}
      </p>
      <ul className={styles.list} aria-labelledby={headingId}>
        {items.map((item) => {
          const field = labelOf(item);
          const { line, column, message } = item.diagnostic;
          return (
            <li key={item.id} className={styles.item}>
              <span className={styles.position}>{t('wl.templating.problems.where', { field, line, column })}</span>
              <span className={styles.message}>{message}</span>
              <button
                type="button"
                className={styles.go}
                aria-label={t('wl.templating.problems.goTo', { field, line, column, message })}
                onClick={() => {
                  onGo(item);
                }}
              >
                {t('wl.editor.errors.go')}
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

export function ProblemList({ problems, onGo, id }: ProblemListProps) {
  const { t } = useTranslation();
  const errors = problems.filter((item) => (item.diagnostic.severity ?? 'error') === 'error');
  const warnings = problems.filter((item) => item.diagnostic.severity === 'warning');
  if (errors.length === 0 && warnings.length === 0) {
    return null;
  }
  return (
    <div id={id} className={styles.problems} role="group" aria-label={t('wl.templating.problems.title')}>
      {errors.length > 0 ? (
        <ProblemGroup
          heading={t('wl.editor.errors.heading', { number: errors.length })}
          tone="error"
          items={errors}
          onGo={onGo}
        />
      ) : null}
      {warnings.length > 0 ? (
        <ProblemGroup
          heading={t('wl.editor.warnings.heading', { number: warnings.length })}
          tone="warning"
          items={warnings}
          onGo={onGo}
        />
      ) : null}
    </div>
  );
}
