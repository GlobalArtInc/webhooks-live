import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useErrorText } from './issueText';
import { useAddEmailAddress, useDeleteEmailAddress } from './notificationsQueries';
import type { EmailAddress } from './types';
import styles from './ChannelEditor.module.css';

export interface EmailAddressesProps {
  addresses: readonly EmailAddress[];
}

interface AddNotice {
  address: string;
  verified: boolean;
}

export function EmailAddresses({ addresses }: EmailAddressesProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const errorText = useErrorText();
  const add = useAddEmailAddress();
  const remove = useDeleteEmailAddress();
  const [draft, setDraft] = useState('');
  const [notice, setNotice] = useState<AddNotice | null>(null);
  const inputId = `${baseId}-address`;

  function submit(event: FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    const address = draft.trim();
    if (address === '') {
      return;
    }
    setNotice(null);
    add.mutate(address, {
      onSuccess: (created) => {
        setNotice({ address: created.address, verified: created.verified });
        setDraft('');
      },
    });
  }

  function addFailure(): string | null {
    if (!add.isError) {
      return null;
    }
    const error = add.error;
    if (error instanceof ApiError && error.code === 'mail_unavailable') {
      return t('wl.notifications.addresses.unavailable');
    }
    if (error instanceof ApiError && error.status === 422) {
      return t('wl.notifications.addresses.invalid');
    }
    return errorText(error);
  }

  const failure = addFailure();

  return (
    <section className={styles.page} aria-labelledby={`${baseId}-heading`}>
      <h3 id={`${baseId}-heading`} className={styles.title}>
        {t('wl.notifications.addresses.title')}
      </h3>
      <p className={styles.hint}>{t('wl.notifications.addresses.intro')}</p>
      {addresses.length === 0 ? (
        <p className={styles.muted}>{t('wl.notifications.addresses.empty')}</p>
      ) : (
        <ul className={styles.addressList} aria-label={t('wl.notifications.addresses.list')}>
          {addresses.map((address) => (
            <li key={address.id} className={styles.addressRow}>
              <span className={styles.addressText}>{address.address}</span>
              <span className={address.verified ? styles.ok : styles.muted}>
                {address.verified ? t('wl.notifications.addresses.verified') : t('wl.notifications.addresses.pending')}
              </span>
              <button
                type="button"
                className={styles.secondary}
                aria-label={t('wl.notifications.addresses.removeFor', { address: address.address })}
                disabled={remove.isPending}
                onClick={() => {
                  remove.mutate(address.id);
                }}
              >
                {t('wl.notifications.addresses.remove')}
              </button>
            </li>
          ))}
        </ul>
      )}
      <form className={styles.addForm} onSubmit={submit}>
        <label htmlFor={inputId} className={styles.label}>
          {t('wl.notifications.addresses.newLabel')}
        </label>
        <div className={styles.addRow}>
          <input
            id={inputId}
            type="email"
            autoComplete="email"
            className={styles.input}
            aria-invalid={failure === null ? undefined : true}
            value={draft}
            onChange={(event) => {
              setDraft(event.target.value);
            }}
          />
          <button type="submit" className={styles.secondary} disabled={add.isPending}>
            {t('wl.notifications.addresses.add')}
          </button>
        </div>
      </form>
      {notice === null ? null : (
        <p role="status" className={styles.hint}>
          {notice.verified
            ? t('wl.notifications.addresses.alreadyVerified', { address: notice.address })
            : t('wl.notifications.addresses.sent', { address: notice.address })}
        </p>
      )}
      {failure === null ? null : (
        <p role="alert" className={styles.error}>
          {failure}
        </p>
      )}
    </section>
  );
}
