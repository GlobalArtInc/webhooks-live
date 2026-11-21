import * as Popover from '@radix-ui/react-popover';
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { filterEntries, groupEntries } from './catalogue';
import styles from './AddStepMenu.module.css';
import type { StepCatalogueEntry, StepClass } from './types';

export interface AddStepMenuProps {
  catalogue: readonly StepCatalogueEntry[];
  onAdd: (kind: string) => void;
  position: number;
  disabled?: boolean;
  disabledReason?: string;
}

export function AddStepMenu({ catalogue, onAdd, position, disabled = false, disabledReason }: AddStepMenuProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const listboxId = `${baseId}-list`;
  const optionRefs = useRef(new Map<StepCatalogueEntry, HTMLElement>());
  const chosenRef = useRef(false);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);

  const visible = useMemo(() => filterEntries(catalogue, query), [catalogue, query]);
  const groups = useMemo(() => groupEntries(visible), [visible]);
  const flat = useMemo(() => groups.flatMap((group) => group.entries), [groups]);
  const positions = useMemo(() => new Map(flat.map((entry, index) => [entry, index])), [flat]);
  const active = flat.length === 0 ? -1 : Math.min(activeIndex, flat.length - 1);
  const activeEntry = active === -1 ? undefined : flat[active];

  function optionId(entry: StepCatalogueEntry): string {
    return `${baseId}-option-${positions.get(entry) ?? 0}`;
  }

  function groupTitle(group: StepClass): string {
    switch (group) {
      case 'response':
        return t('wl.steps.class.response');
      case 'compute':
        return t('wl.steps.class.compute');
      case 'state':
        return t('wl.steps.class.state');
      case 'fetch':
        return t('wl.steps.class.fetch');
      case 'effect':
        return t('wl.steps.class.effect');
    }
  }

  useEffect(() => {
    if (activeEntry !== undefined) {
      optionRefs.current.get(activeEntry)?.scrollIntoView?.({ block: 'nearest' });
    }
  }, [activeEntry]);

  function choose(entry: StepCatalogueEntry): void {
    chosenRef.current = true;
    setOpen(false);
    onAdd(entry.kind);
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

  let resultText = t('wl.steps.menu.results', { count: flat.length });
  if (flat.length === 0) {
    resultText = t('wl.steps.menu.empty');
  }

  return (
    <Popover.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) {
          chosenRef.current = false;
          setQuery('');
          setActiveIndex(0);
        }
      }}
    >
      <Popover.Trigger asChild>
        <button
          type="button"
          className={styles.trigger}
          data-add-step=""
          disabled={disabled}
          aria-label={t('wl.steps.menu.trigger', { position })}
          title={disabled ? disabledReason : undefined}
        >
          <span aria-hidden="true">+ </span>
          <span>{t('wl.steps.menu.label')}</span>
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className={styles.content}
          align="start"
          sideOffset={4}
          collisionPadding={8}
          aria-label={t('wl.steps.menu.title')}
          onCloseAutoFocus={(event) => {
            if (chosenRef.current) {
              event.preventDefault();
            }
          }}
        >
          <input
            type="text"
            role="combobox"
            className={styles.filter}
            aria-label={t('wl.steps.menu.filter')}
            aria-expanded="true"
            aria-controls={listboxId}
            aria-autocomplete="list"
            aria-activedescendant={activeEntry === undefined ? undefined : optionId(activeEntry)}
            placeholder={t('wl.steps.menu.filterPlaceholder')}
            autoComplete="off"
            spellCheck={false}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActiveIndex(0);
            }}
            onKeyDown={handleKeyDown}
          />
          <div id={listboxId} role="listbox" aria-label={t('wl.steps.menu.title')} className={styles.list}>
            {groups.map((group) => {
              const headingId = `${baseId}-group-${group.id}`;
              return (
                <div key={group.id} role="group" aria-labelledby={headingId}>
                  <div id={headingId} className={styles.groupTitle}>
                    {groupTitle(group.id)}
                  </div>
                  {group.entries.map((entry) => (
                    <div
                      key={entry.kind}
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
                      <span className={styles.name}>{entry.title}</span>
                      <span className={styles.description}>{entry.description}</span>
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
          <p className={styles.status} role="status">
            {resultText}
          </p>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
