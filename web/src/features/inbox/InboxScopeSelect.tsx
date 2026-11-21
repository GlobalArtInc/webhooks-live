import type { Ref } from 'react';
import { useTranslation } from 'react-i18next';
import type { Folder } from '../../api/types';
import type { MyEndpoint } from '../endpoints/storage';
import { shortEndpointId } from '../endpoints/format';
import { byRecentlyOpened } from '../endpoints/storage';
import { endpointLimit, parseInboxScope, selectValueOf, type InboxScope } from './scope';
import styles from './InboxScopeSelect.module.css';

export interface InboxScopeSelectProps {
  scope: InboxScope;
  signedIn: boolean;
  folders: readonly Folder[];
  local: readonly MyEndpoint[];
  selectRef: Ref<HTMLSelectElement>;
  onChange: (scope: InboxScope) => void;
}

interface ChecklistProps {
  chosen: readonly string[];
  signedIn: boolean;
  local: readonly MyEndpoint[];
  onToggle: (endpointId: string) => void;
}

function EndpointChecklist({ chosen, signedIn, local, onToggle }: ChecklistProps) {
  const { t } = useTranslation();
  const max = endpointLimit(signedIn);
  const entries = byRecentlyOpened(local);
  return (
    <fieldset className={styles.checklist}>
      <legend className={styles.legend}>{t('wl.inbox.scope.endpoints.legend')}</legend>
      <p className={styles.hint}>{t('wl.inbox.scope.endpoints.limit', { max })}</p>
      {entries.length === 0 ? <p className={styles.hint}>{t('wl.inbox.scope.endpoints.none')}</p> : null}
      {entries.map((entry) => {
        const selected = chosen.includes(entry.id);
        return (
          <label key={entry.id} className={styles.option}>
            <input
              type="checkbox"
              checked={selected}
              disabled={!selected && chosen.length >= max}
              onChange={() => {
                onToggle(entry.id);
              }}
            />
            <span className={styles.name}>{entry.label ?? shortEndpointId(entry.id)}</span>
          </label>
        );
      })}
    </fieldset>
  );
}

export function InboxScopeSelect({ scope, signedIn, folders, local, selectRef, onChange }: InboxScopeSelectProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.root}>
      <label className={styles.field}>
        <span className={styles.label}>{t('wl.inbox.scope.label')}</span>
        <select
          ref={selectRef}
          className={styles.select}
          value={selectValueOf(scope)}
          onChange={(event) => {
            onChange(parseInboxScope(event.target.value));
          }}
        >
          <option value="all">{signedIn ? t('wl.inbox.scope.all') : t('wl.inbox.scope.allLocal')}</option>
          {folders.map((folder) => (
            <option key={folder.id} value={`folder:${folder.id}`}>
              {folder.name}
            </option>
          ))}
          <option value="endpoints">{t('wl.inbox.scope.endpoints')}</option>
        </select>
      </label>
      {scope.kind === 'endpoints' ? (
        <EndpointChecklist
          chosen={scope.endpointIds}
          signedIn={signedIn}
          local={local}
          onToggle={(endpointId) => {
            const next = scope.endpointIds.includes(endpointId)
              ? scope.endpointIds.filter((id) => id !== endpointId)
              : [...scope.endpointIds, endpointId];
            onChange({ kind: 'endpoints', endpointIds: next });
          }}
        />
      ) : null}
    </div>
  );
}
