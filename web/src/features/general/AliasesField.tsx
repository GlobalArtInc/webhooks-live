import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useAddAlias, useAliases, useRemoveAlias, type Alias } from '../../api/general';
import { ApiError } from '../../api/client';
import { offersAliases } from '../../api/capabilities';
import { useInstance } from '../../api/queries';
import { CopyButton } from '../../components/CopyButton/CopyButton';
import { useAliasAvailability, type AliasStatus } from './useAliasAvailability';
import styles from './AliasesField.module.css';

export interface AliasesFieldProps {
  endpointId: string;
  readOnly: boolean;
}

type AliasStatusKey =
  | 'wl.general.aliases.status.checking'
  | 'wl.general.aliases.status.error'
  | 'wl.general.aliases.status.available'
  | 'wl.general.aliases.status.taken'
  | 'wl.general.aliases.status.reserved'
  | 'wl.general.aliases.status.invalid';

function statusKey(status: AliasStatus): AliasStatusKey | null {
  if (status.state === 'idle') {
    return null;
  }
  if (status.state === 'checking') {
    return 'wl.general.aliases.status.checking';
  }
  if (status.state === 'error') {
    return 'wl.general.aliases.status.error';
  }
  const { available, reason } = status.availability;
  if (available) {
    return 'wl.general.aliases.status.available';
  }
  if (reason === 'taken') {
    return 'wl.general.aliases.status.taken';
  }
  if (reason === 'reserved') {
    return 'wl.general.aliases.status.reserved';
  }
  return 'wl.general.aliases.status.invalid';
}

function isAvailable(status: AliasStatus): boolean {
  return status.state === 'ready' && status.availability.available;
}

export function AliasesField({ endpointId, readOnly }: AliasesFieldProps) {
  const { t } = useTranslation();
  const inputId = useId();
  const statusId = useId();
  const instance = useInstance().data;
  const offered = offersAliases(instance);
  const aliases = useAliases(endpointId, offered);
  const add = useAddAlias(endpointId);
  const remove = useRemoveAlias(endpointId);
  const [candidate, setCandidate] = useState('');
  const [message, setMessage] = useState('');
  const status = useAliasAvailability(candidate);
  const alias = candidate.trim();
  const canAdd = !readOnly && isAvailable(status) && !add.isPending;

  function submit(event: FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    if (!canAdd) {
      return;
    }
    add.mutate(alias, {
      onSuccess: () => {
        setCandidate('');
        setMessage(t('wl.general.aliases.added'));
      },
      onError: (error) => {
        if (error instanceof ApiError && error.code === 'limit_reached') {
          setMessage(t('wl.general.aliases.limit', { count: error.limit?.max ?? 0 }));
          return;
        }
        if (error instanceof ApiError && error.code === 'alias_taken') {
          setMessage(t('wl.general.aliases.status.taken'));
          return;
        }
        setMessage(t('wl.general.aliases.addFailed'));
      },
    });
  }

  function removeAlias(item: Alias): void {
    setMessage('');
    remove.mutate(item.alias, {
      onSuccess: () => {
        setMessage(t('wl.general.aliases.removed'));
      },
      onError: () => {
        setMessage(t('wl.general.aliases.removeFailed'));
      },
    });
  }

  const items = aliases.data?.items ?? [];
  const checkKey = readOnly ? null : statusKey(status);
  const checkText = checkKey === null ? '' : t(checkKey);

  return (
    <section className={styles.group} aria-labelledby={`${inputId}-heading`}>
      <h3 id={`${inputId}-heading`} className={styles.heading}>
        {t('wl.general.aliases.title')}
      </h3>
      <p className={styles.help}>{t('wl.general.aliases.help')}</p>
      {instance !== undefined && !offered ? <p className={styles.help}>{t('wl.general.aliases.unavailable')}</p> : null}
      {offered && readOnly ? <p className={styles.help}>{t('wl.general.aliases.ownerOnly')}</p> : null}
      {aliases.isError ? (
        <p role="alert" className={styles.error}>
          {t('wl.general.aliases.failed')}
        </p>
      ) : null}
      {aliases.data !== undefined && items.length === 0 ? (
        <p className={styles.help}>{t('wl.general.aliases.empty')}</p>
      ) : null}
      {items.length > 0 ? (
        <ul className={styles.list}>
          {items.map((item) => (
            <li key={item.alias} className={styles.row}>
              <span className={styles.url}>{item.url}</span>
              <span className={styles.rowActions}>
                <CopyButton value={item.url} label={t('wl.general.aliases.copyNamed', { alias: item.alias })} />
                {readOnly ? null : (
                  <button
                    type="button"
                    className={styles.secondary}
                    aria-label={t('wl.general.aliases.removeNamed', { alias: item.alias })}
                    disabled={remove.isPending}
                    onClick={() => {
                      removeAlias(item);
                    }}
                  >
                    {t('wl.general.aliases.remove')}
                  </button>
                )}
              </span>
            </li>
          ))}
        </ul>
      ) : null}
      {readOnly || !offered ? null : (
        <form className={styles.form} onSubmit={submit}>
          <div className={styles.field}>
            <label htmlFor={inputId} className={styles.label}>
              {t('wl.general.aliases.label')}
            </label>
            <input
              id={inputId}
              type="text"
              autoComplete="off"
              spellCheck={false}
              className={styles.input}
              value={candidate}
              aria-describedby={statusId}
              onChange={(event) => {
                setCandidate(event.target.value);
              }}
            />
            <p id={statusId} role="status" className={styles.status}>
              {checkText}
            </p>
          </div>
          <button type="submit" className={styles.primary} disabled={!canAdd}>
            {t('wl.general.aliases.add')}
          </button>
        </form>
      )}
      {message === '' ? null : (
        <p role="status" className={styles.help}>
          {message}
        </p>
      )}
    </section>
  );
}
