import * as Popover from '@radix-ui/react-popover';
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './VariablePicker.module.css';
import { filterVariables, groupVariables, type VariableEntry } from './variables';

export interface VariablePickerProps {
  catalogue: readonly VariableEntry[];
  onInsert: (entry: VariableEntry) => void;
  sampleCapture?: string;
  loading?: boolean;
  disabled?: boolean;
  compact?: boolean;
  triggerLabel?: string;
  onCloseFocus?: () => void;
}

export function VariablePicker({
  catalogue,
  onInsert,
  sampleCapture,
  loading = false,
  disabled = false,
  compact = false,
  triggerLabel,
  onCloseFocus,
}: VariablePickerProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const listboxId = `${baseId}-list`;
  const optionRefs = useRef(new Map<VariableEntry, HTMLElement>());
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);

  const visible = useMemo(() => filterVariables(catalogue, query), [catalogue, query]);
  const groups = useMemo(() => groupVariables(visible), [visible]);
  const flat = useMemo(() => groups.flatMap((group) => group.entries), [groups]);
  const positions = useMemo(() => new Map(flat.map((entry, index) => [entry, index])), [flat]);
  const active = flat.length === 0 ? -1 : Math.min(activeIndex, flat.length - 1);
  const activeEntry = active === -1 ? undefined : flat[active];
  const insertLabel = triggerLabel ?? t('wl.variables.insert');

  function optionId(entry: VariableEntry): string {
    return `${baseId}-option-${positions.get(entry) ?? 0}`;
  }

  function groupTitle(group: string): string {
    switch (group) {
      case 'request':
        return t('wl.variables.group.request');
      case 'time':
        return t('wl.variables.group.time');
      case 'rules':
        return t('wl.variables.group.rules');
      case 'helpers':
        return t('wl.variables.group.helpers');
      default:
        return group;
    }
  }

  function isKnownGroup(group: string): boolean {
    return group === 'request' || group === 'time' || group === 'rules' || group === 'helpers';
  }

  useEffect(() => {
    if (activeEntry !== undefined) {
      optionRefs.current.get(activeEntry)?.scrollIntoView?.({ block: 'nearest' });
    }
  }, [activeEntry]);

  function choose(entry: VariableEntry): void {
    setOpen(false);
    onInsert(entry);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>): void {
    if (event.key === 'ArrowDown' && flat.length > 0) {
      event.preventDefault();
      setActiveIndex(Math.min(active + 1, flat.length - 1));
    } else if (event.key === 'ArrowUp' && flat.length > 0) {
      event.preventDefault();
      setActiveIndex(Math.max(active - 1, 0));
    } else if (event.key === 'Enter' && activeEntry !== undefined) {
      event.preventDefault();
      choose(activeEntry);
    }
  }

  let resultText = t('wl.variables.results', { count: flat.length });
  if (loading) {
    resultText = t('wl.variables.loading');
  } else if (flat.length === 0) {
    resultText = t('wl.variables.empty');
  }

  return (
    <Popover.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) {
          setQuery('');
          setActiveIndex(0);
        }
      }}
    >
      <Popover.Trigger asChild>
        <button
          type="button"
          className={styles.trigger}
          disabled={disabled}
          aria-label={compact ? insertLabel : undefined}
        >
          <span aria-hidden="true">{'{}'}</span>
          {compact ? null : <span> {insertLabel}</span>}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className={styles.content}
          align="start"
          sideOffset={4}
          collisionPadding={8}
          aria-label={t('wl.variables.title')}
          onCloseAutoFocus={(event) => {
            if (onCloseFocus !== undefined) {
              event.preventDefault();
              onCloseFocus();
            }
          }}
        >
          <div className={styles.header}>
            <input
              type="text"
              role="combobox"
              className={styles.filter}
              aria-label={t('wl.variables.filter')}
              aria-expanded="true"
              aria-controls={listboxId}
              aria-autocomplete="list"
              aria-activedescendant={activeEntry === undefined ? undefined : optionId(activeEntry)}
              placeholder={t('wl.variables.filterPlaceholder')}
              autoComplete="off"
              spellCheck={false}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActiveIndex(0);
              }}
              onKeyDown={handleKeyDown}
            />
            {sampleCapture === undefined ? null : (
              <span className={styles.sample}>{t('wl.variables.sample', { request: sampleCapture })}</span>
            )}
          </div>
          <div
            id={listboxId}
            role="listbox"
            aria-label={t('wl.variables.title')}
            aria-busy={loading}
            className={styles.list}
          >
            {groups.map((group) => {
              const headingId = `${baseId}-group-${group.id}`;
              return (
                <div key={group.id} role="group" aria-labelledby={headingId}>
                  <div id={headingId} className={styles.groupTitle} lang={isKnownGroup(group.id) ? undefined : 'en'}>
                    {groupTitle(group.id)}
                  </div>
                  {group.entries.map((entry) => (
                    <div
                      key={entry.path}
                      id={optionId(entry)}
                      role="option"
                      aria-selected={entry === activeEntry}
                      className={styles.option}
                      ref={(element) => {
                        if (element === null) {
                          optionRefs.current.delete(entry);
                        } else {
                          optionRefs.current.set(entry, element);
                        }
                      }}
                      onClick={() => {
                        choose(entry);
                      }}
                    >
                      <span className={styles.path}>{entry.path}</span>
                      {entry.type === undefined ? null : <span className={styles.type}>{entry.type}</span>}
                      {entry.sample === undefined ? null : <span className={styles.value}>{entry.sample}</span>}
                      {entry.description === undefined ? null : (
                        <span className={styles.description}>{entry.description}</span>
                      )}
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
          <p className={styles.status} role="status">
            {resultText}
          </p>
          <p className={styles.hint}>{t('wl.variables.hint')}</p>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
