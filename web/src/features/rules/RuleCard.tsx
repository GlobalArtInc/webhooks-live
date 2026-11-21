import { useSortable } from '@dnd-kit/sortable';
import { Link } from '@tanstack/react-router';
import type { CSSProperties, KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import type { Rule } from '../../api/rulesTypes';
import type { MoveControl } from './order';
import styles from './RuleCard.module.css';

export interface RuleCardProps {
  endpointId: string;
  rule: Rule;
  position: number;
  total: number;
  readOnly: boolean;
  busy: boolean;
  onToggle: (rule: Rule) => void;
  onMove: (ruleId: string, control: MoveControl) => void;
  onDelete: (rule: Rule) => void;
}

export function RuleCard({
  endpointId,
  rule,
  position,
  total,
  readOnly,
  busy,
  onToggle,
  onMove,
  onDelete,
}: RuleCardProps) {
  const { t } = useTranslation();
  const { setNodeRef, setActivatorNodeRef, listeners, transform, isDragging } = useSortable({
    id: rule.id,
    disabled: readOnly,
  });
  const itemStyle: CSSProperties | undefined =
    transform === null ? undefined : { transform: `translate3d(${transform.x}px, ${transform.y}px, 0)` };
  const dragListeners = readOnly ? undefined : listeners;
  const name = rule.name;
  const cardClass = [styles.card, rule.enabled ? '' : styles.off, isDragging ? styles.dragging : '']
    .filter((value) => value !== '')
    .join(' ');
  const switchClass = rule.enabled ? `${styles.switch} ${styles.on}` : styles.switch;

  function handleKeyDown(event: KeyboardEvent<HTMLElement>): void {
    if (readOnly || !event.altKey || !event.shiftKey) {
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      onMove(rule.id, 'up');
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      onMove(rule.id, 'down');
    }
  }

  return (
    <li ref={setNodeRef} style={itemStyle} className={styles.item}>
      <section
        aria-label={t('wl.rules.card.label', { position, total, name })}
        className={cardClass}
        onKeyDown={handleKeyDown}
      >
        <div className={styles.head}>
          <span ref={setActivatorNodeRef} className={styles.handle} aria-hidden="true" {...dragListeners}>
            ::
          </span>
          <Link
            to="/e/$id/settings/rules/$ruleId"
            params={{ id: endpointId, ruleId: rule.id }}
            className={styles.name}
            data-focus-key={`${rule.id}:name`}
          >
            {name}
          </Link>
          <button
            type="button"
            role="switch"
            aria-checked={rule.enabled}
            aria-label={t('wl.rules.card.enabledFor', { name })}
            className={switchClass}
            disabled={readOnly || busy}
            data-focus-key={`${rule.id}:switch`}
            onClick={() => {
              onToggle(rule);
            }}
          >
            <span className={styles.knob} aria-hidden="true" />
            <span>{rule.enabled ? t('wl.rules.state.on') : t('wl.rules.state.off')}</span>
          </button>
        </div>
        <p className={styles.meta}>{t('wl.rules.card.steps', { count: rule.steps.length })}</p>
        {readOnly ? null : (
          <div className={styles.actions}>
            <button
              type="button"
              className={styles.button}
              aria-label={t('wl.rules.card.moveUp', { name })}
              disabled={busy || position <= 1}
              data-focus-key={`${rule.id}:up`}
              onClick={() => {
                onMove(rule.id, 'up');
              }}
            >
              {t('wl.rules.card.up')}
            </button>
            <button
              type="button"
              className={styles.button}
              aria-label={t('wl.rules.card.moveDown', { name })}
              disabled={busy || position >= total}
              data-focus-key={`${rule.id}:down`}
              onClick={() => {
                onMove(rule.id, 'down');
              }}
            >
              {t('wl.rules.card.down')}
            </button>
            <button
              type="button"
              className={styles.button}
              aria-label={t('wl.rules.card.moveTop', { name })}
              disabled={busy || position <= 1}
              data-focus-key={`${rule.id}:top`}
              onClick={() => {
                onMove(rule.id, 'top');
              }}
            >
              {t('wl.rules.card.top')}
            </button>
            <Link
              to="/e/$id/settings/rules/$ruleId"
              params={{ id: endpointId, ruleId: 'new' }}
              search={{ from: rule.id }}
              className={styles.button}
              aria-label={t('wl.rules.card.duplicateFor', { name })}
            >
              {t('wl.rules.card.duplicate')}
            </Link>
            <button
              type="button"
              className={styles.danger}
              aria-label={t('wl.rules.card.deleteFor', { name })}
              disabled={busy}
              onClick={() => {
                onDelete(rule);
              }}
            >
              {t('wl.rules.card.delete')}
            </button>
          </div>
        )}
      </section>
    </li>
  );
}
