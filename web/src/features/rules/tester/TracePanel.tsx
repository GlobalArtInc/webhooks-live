import { useTranslation } from 'react-i18next';
import { isRevealed } from './cursor';
import styles from './Tester.module.css';
import { TraceEntry } from './TraceEntry';
import type { Cursor, ErrorPosition, TraceItem, TraceModel, VariableRow } from './types';

export interface TracePanelProps {
  trace: TraceModel;
  cursor: Cursor;
  showRule: boolean;
  stepTitles: Readonly<Record<string, string>> | undefined;
  changes: readonly VariableRow[];
  onSelect: (index: number) => void;
  onGoTo: ((item: TraceItem, position: ErrorPosition) => void) | undefined;
}

export function TracePanel({ trace, cursor, showRule, stepTitles, changes, onSelect, onGoTo }: TracePanelProps) {
  const { t } = useTranslation();
  return (
    <div>
      <h3 className={styles.sectionTitle}>{t('wl.tester.trace.title')}</h3>
      <ol className={styles.trace} aria-label={t('wl.tester.trace.label')}>
        {trace.items.map((item, index) => (
          <TraceEntry
            key={item.id}
            item={item}
            index={index}
            selected={index === cursor.selected}
            revealed={isRevealed(cursor, index)}
            showRule={showRule}
            stepTitles={stepTitles}
            changes={index === cursor.selected ? changes : []}
            onSelect={onSelect}
            onGoTo={onGoTo}
          />
        ))}
      </ol>
      {trace.truncated ? <p className={styles.note}>{t('wl.tester.trace.truncated')}</p> : null}
    </div>
  );
}
