import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  isRangeOption,
  KIND_OPTIONS,
  MAX_TEXT_LENGTH,
  METHOD_OPTIONS,
  RANGE_OPTIONS,
  STATUS_OPTIONS,
  toggleToken,
  type CaptureSearch,
  type CaptureSearchPatch,
  type RangeOption,
} from '../filters/captureSearch';
import { filterStringOf, normalisedSearch, patchFor, searchOfFilterString } from './filterString';
import { SavedFilters } from './SavedFiltersSection';
import styles from './FilterBuilder.module.css';

export interface BuilderFormProps {
  endpointId: string;
  search: CaptureSearch;
  onApply: (patch: CaptureSearchPatch) => void;
}

function emptyToUndefined(text: string): string | undefined {
  return text === '' ? undefined : text;
}

interface ChoiceGroupProps {
  legend: string;
  options: readonly string[];
  selected: string | undefined;
  labelOf: (option: string) => string;
  onToggle: (option: string) => void;
}

function ChoiceGroup({ legend, options, selected, labelOf, onToggle }: ChoiceGroupProps) {
  const chosen = selected?.split(',') ?? [];
  return (
    <fieldset className={styles.choices}>
      <legend className={styles.legend}>{legend}</legend>
      {options.map((option) => (
        <label key={option} className={styles.choice}>
          <input
            type="checkbox"
            checked={chosen.includes(option)}
            onChange={() => {
              onToggle(option);
            }}
          />
          <span>{labelOf(option)}</span>
        </label>
      ))}
    </fieldset>
  );
}

interface TextFieldProps {
  id: string;
  label: string;
  hint: string;
  value: string | undefined;
  placeholder: string;
  maxLength: number;
  error?: string;
  onChange: (value: string) => void;
}

function TextField({ id, label, hint, value, placeholder, maxLength, error, onChange }: TextFieldProps) {
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  return (
    <div className={styles.field}>
      <label htmlFor={id} className={styles.label}>
        {label}
      </label>
      <input
        id={id}
        type="text"
        className={styles.input}
        value={value ?? ''}
        placeholder={placeholder}
        maxLength={maxLength}
        aria-invalid={error !== undefined}
        aria-describedby={error === undefined ? hintId : `${hintId} ${errorId}`}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
      <span id={hintId} className={styles.hint}>
        {hint}
      </span>
      {error === undefined ? null : (
        <span id={errorId} role="alert" className={styles.error}>
          {error}
        </span>
      )}
    </div>
  );
}

export function BuilderForm({ endpointId, search, onApply }: BuilderFormProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const [draft, setDraft] = useState<CaptureSearch>(search);
  const [senderInvalid, setSenderInvalid] = useState(false);

  function update(patch: CaptureSearchPatch) {
    setDraft((current) => ({ ...current, ...patch }));
  }

  function resolve(): CaptureSearch | null {
    const next = normalisedSearch(draft, Date.now());
    const invalid = draft.ip !== undefined && draft.ip.trim() !== '' && next.ip === undefined;
    setSenderInvalid(invalid);
    return invalid ? null : next;
  }

  function apply() {
    const next = resolve();
    if (next !== null) {
      onApply(patchFor(next));
    }
  }

  function clear() {
    setDraft({});
    setSenderInvalid(false);
  }

  function chooseRange(value: string) {
    update({ range: isRangeOption(value) ? value : undefined, since: undefined });
  }

  function kindLabel(option: string): string {
    switch (option) {
      case 'email':
        return t('wl.filterbuilder.kind.email');
      case 'dns':
        return t('wl.filterbuilder.kind.dns');
      default:
        return t('wl.filterbuilder.kind.http');
    }
  }

  function rangeLabel(range: RangeOption): string {
    switch (range) {
      case '15m':
        return t('wl.filterbuilder.time.last15m');
      case '1h':
        return t('wl.filterbuilder.time.last1h');
      case '24h':
        return t('wl.filterbuilder.time.last24h');
      case '7d':
        return t('wl.filterbuilder.time.last7d');
    }
  }

  return (
    <div className={styles.body}>
      <TextField
        id={`${baseId}-text`}
        label={t('wl.filterbuilder.text.label')}
        hint={t('wl.filterbuilder.text.hint')}
        value={draft.q}
        placeholder={t('wl.filterbuilder.text.placeholder')}
        maxLength={MAX_TEXT_LENGTH}
        onChange={(value) => {
          update({ q: emptyToUndefined(value) });
        }}
      />
      <ChoiceGroup
        legend={t('wl.filterbuilder.kind.legend')}
        options={KIND_OPTIONS}
        selected={draft.kind}
        labelOf={kindLabel}
        onToggle={(option) => {
          update({ kind: toggleToken(draft.kind, option) });
        }}
      />
      <ChoiceGroup
        legend={t('wl.filterbuilder.method.legend')}
        options={METHOD_OPTIONS}
        selected={draft.method}
        labelOf={(option) => option}
        onToggle={(option) => {
          update({ method: toggleToken(draft.method, option) });
        }}
      />
      <ChoiceGroup
        legend={t('wl.filterbuilder.status.legend')}
        options={STATUS_OPTIONS}
        selected={draft.status}
        labelOf={(option) => (option === 'none' ? t('wl.filterbuilder.status.none') : option)}
        onToggle={(option) => {
          update({ status: toggleToken(draft.status, option) });
        }}
      />
      <div className={styles.field}>
        <label htmlFor={`${baseId}-time`} className={styles.label}>
          {t('wl.filterbuilder.time.label')}
        </label>
        <select
          id={`${baseId}-time`}
          className={styles.input}
          value={draft.range ?? ''}
          onChange={(event) => {
            chooseRange(event.target.value);
          }}
        >
          <option value="">{t('wl.filterbuilder.time.any')}</option>
          {RANGE_OPTIONS.map((range) => (
            <option key={range} value={range}>
              {rangeLabel(range)}
            </option>
          ))}
        </select>
      </div>
      <TextField
        id={`${baseId}-sender`}
        label={t('wl.filterbuilder.sender.label')}
        hint={t('wl.filterbuilder.sender.hint')}
        value={draft.ip}
        placeholder={t('wl.filterbuilder.sender.placeholder')}
        maxLength={MAX_TEXT_LENGTH}
        error={senderInvalid ? t('wl.filterbuilder.sender.invalid') : undefined}
        onChange={(value) => {
          update({ ip: emptyToUndefined(value) });
        }}
      />
      <TextField
        id={`${baseId}-path`}
        label={t('wl.filterbuilder.path.label')}
        hint={t('wl.filterbuilder.path.hint')}
        value={draft.path}
        placeholder={t('wl.filterbuilder.path.placeholder')}
        maxLength={MAX_TEXT_LENGTH}
        onChange={(value) => {
          update({ path: emptyToUndefined(value) });
        }}
      />
      <div className={styles.actions}>
        <button type="button" className={styles.secondary} onClick={clear}>
          {t('wl.filterbuilder.clear')}
        </button>
        <button type="button" className={styles.primary} onClick={apply}>
          {t('wl.filterbuilder.apply')}
        </button>
      </div>
      <SavedFilters
        endpointId={endpointId}
        filterFor={() => {
          const next = resolve();
          return next === null ? null : filterStringOf(next);
        }}
        onOpen={(filter) => {
          onApply(patchFor(searchOfFilterString(filter, Date.now())));
        }}
      />
    </div>
  );
}
