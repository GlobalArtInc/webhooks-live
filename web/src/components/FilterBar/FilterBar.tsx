import { useEffect, useId, useRef, useState, type ChangeEvent, type Ref } from 'react';
import { useTranslation } from 'react-i18next';
import type { ViewportLayout } from '../../features/endpoint/useViewport';
import { FilterBuilder } from '../../features/filterbuilder';
import {
  activePartCount,
  hasCaptureSearch,
  isRangeOption,
  KIND_OPTIONS,
  METHOD_OPTIONS,
  MAX_TEXT_LENGTH,
  RANGE_OPTIONS,
  rangeSince,
  STATUS_OPTIONS,
  toggleToken,
  type CaptureSearch,
  type CaptureSearchPatch,
  type RangeOption,
} from '../../features/filters/captureSearch';
import styles from './FilterBar.module.css';

export const SEARCH_DEBOUNCE_MS = 300;

type Timer = ReturnType<typeof setTimeout>;

export interface FilterBarProps {
  search: CaptureSearch;
  layout: ViewportLayout;
  hiddenCount: number;
  onChange: (patch: CaptureSearchPatch) => void;
  onClear: () => void;
  searchRef?: Ref<HTMLInputElement>;
}

function textOrUndefined(text: string): string | undefined {
  const trimmed = text.trim();
  return trimmed === '' ? undefined : trimmed;
}

interface SearchFieldProps {
  value: string | undefined;
  onCommit: (text: string | undefined) => void;
  inputRef?: Ref<HTMLInputElement>;
}

function SearchField({ value, onCommit, inputRef }: SearchFieldProps) {
  const { t } = useTranslation();
  const [text, setText] = useState(value ?? '');
  const committed = useRef(value);
  const timer = useRef<Timer | null>(null);

  useEffect(() => {
    if (value !== committed.current) {
      committed.current = value;
      setText(value ?? '');
    }
  }, [value]);

  useEffect(
    () => () => {
      if (timer.current !== null) {
        clearTimeout(timer.current);
      }
    },
    [],
  );

  function handleChange(event: ChangeEvent<HTMLInputElement>) {
    const next = event.target.value;
    setText(next);
    if (timer.current !== null) {
      clearTimeout(timer.current);
    }
    timer.current = setTimeout(() => {
      timer.current = null;
      const query = textOrUndefined(next);
      committed.current = query;
      onCommit(query);
    }, SEARCH_DEBOUNCE_MS);
  }

  return (
    <input
      ref={inputRef}
      type="search"
      className={styles.search}
      aria-label={t('wl.endpoint.filter.search.label')}
      placeholder={t('wl.endpoint.filter.search.placeholder')}
      maxLength={MAX_TEXT_LENGTH}
      value={text}
      onChange={handleChange}
    />
  );
}

interface ChipGroupProps {
  label: string;
  options: readonly string[];
  selected: string | undefined;
  labelOf: (option: string) => string;
  onToggle: (option: string) => void;
}

function ChipGroup({ label, options, selected, labelOf, onToggle }: ChipGroupProps) {
  const chosen = selected?.split(',') ?? [];
  return (
    <div role="group" aria-label={label} className={styles.chips}>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          className={styles.chip}
          aria-pressed={chosen.includes(option)}
          onClick={() => {
            onToggle(option);
          }}
        >
          {labelOf(option)}
        </button>
      ))}
    </div>
  );
}

export function FilterBar({ search, layout, hiddenCount, onChange, onClear, searchRef }: FilterBarProps) {
  const { t } = useTranslation();
  const panelId = useId();
  const [open, setOpen] = useState(false);
  const [fieldGeneration, setFieldGeneration] = useState(0);
  const collapsible = layout !== 'wide';
  const expanded = !collapsible || open;
  const hasFilters = hasCaptureSearch(search);

  function statusLabel(option: string): string {
    return option === 'none' ? t('wl.endpoint.filter.statusNone') : option;
  }

  function kindLabel(option: string): string {
    switch (option) {
      case 'email':
        return t('wl.kinds.filter.email');
      case 'dns':
        return t('wl.kinds.filter.dns');
      default:
        return t('wl.kinds.filter.http');
    }
  }

  function rangeLabel(range: RangeOption): string {
    switch (range) {
      case '15m':
        return t('wl.endpoint.filter.range.last15m');
      case '1h':
        return t('wl.endpoint.filter.range.last1h');
      case '24h':
        return t('wl.endpoint.filter.range.last24h');
      case '7d':
        return t('wl.endpoint.filter.range.last7d');
    }
  }

  function chooseRange(value: string) {
    if (isRangeOption(value)) {
      onChange({ range: value, since: rangeSince(value, Date.now()) });
      return;
    }
    onChange({ range: undefined, since: undefined });
  }

  function clearAll() {
    setFieldGeneration((generation) => generation + 1);
    onClear();
  }

  return (
    <div role="search" className={styles.root} data-layout={layout}>
      <div className={styles.row}>
        <SearchField
          key={fieldGeneration}
          inputRef={searchRef}
          value={search.q}
          onCommit={(q) => {
            onChange({ q });
          }}
        />
        <FilterBuilder search={search} layout={layout} onApply={onChange} />
        {collapsible ? (
          <button
            type="button"
            className={styles.button}
            aria-expanded={expanded}
            aria-controls={panelId}
            onClick={() => {
              setOpen((current) => !current);
            }}
          >
            {t('wl.endpoint.filter.toggle', { count: activePartCount(search) })}
          </button>
        ) : null}
      </div>
      <div
        id={panelId}
        role="group"
        aria-label={t('wl.endpoint.filter.panel')}
        className={styles.panel}
        hidden={!expanded}
      >
        <ChipGroup
          label={t('wl.kinds.filter.label')}
          options={KIND_OPTIONS}
          selected={search.kind}
          labelOf={kindLabel}
          onToggle={(option) => {
            onChange({ kind: toggleToken(search.kind, option) });
          }}
        />
        <ChipGroup
          label={t('wl.endpoint.filter.method')}
          options={METHOD_OPTIONS}
          selected={search.method}
          labelOf={(option) => option}
          onToggle={(option) => {
            onChange({ method: toggleToken(search.method, option) });
          }}
        />
        <ChipGroup
          label={t('wl.endpoint.filter.status')}
          options={STATUS_OPTIONS}
          selected={search.status}
          labelOf={statusLabel}
          onToggle={(option) => {
            onChange({ status: toggleToken(search.status, option) });
          }}
        />
        <label className={styles.field}>
          <span>{t('wl.endpoint.filter.time')}</span>
          <select
            className={styles.select}
            value={search.range ?? ''}
            onChange={(event) => {
              chooseRange(event.target.value);
            }}
          >
            <option value="">{t('wl.endpoint.filter.range.any')}</option>
            {RANGE_OPTIONS.map((range) => (
              <option key={range} value={range}>
                {rangeLabel(range)}
              </option>
            ))}
          </select>
        </label>
        <button type="button" className={styles.button} disabled={!hasFilters} onClick={clearAll}>
          {t('wl.endpoint.filter.clear')}
        </button>
      </div>
      {hiddenCount > 0 ? (
        <div className={styles.hidden}>
          <span>{t('wl.endpoint.filter.hidden', { count: hiddenCount })}</span>
          <button type="button" className={styles.button} onClick={clearAll}>
            {t('wl.endpoint.filter.showAll')}
          </button>
        </div>
      ) : null}
    </div>
  );
}
