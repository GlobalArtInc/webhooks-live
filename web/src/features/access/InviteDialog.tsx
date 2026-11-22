import * as Dialog from '@radix-ui/react-dialog';
import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import type { MemberRole } from '../../api/types';
import dialogStyles from '../../components/ConfirmDialog/ConfirmDialog.module.css';
import { useReturnFocus } from '../../components/useReturnFocus';
import { usePutMember } from './useMembers';
import styles from './access.module.css';

export interface InviteDialogProps {
  endpointId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export type InviteFailure = 'email' | 'unknown' | 'owner' | 'forbidden' | 'limited' | 'failed';

const DEFAULT_WAIT_SECONDS = 30;
const EMAIL_CODES: readonly string[] = ['invalid_format', 'required', 'too_long'];

const EMAIL_SHAPE = /^[^\s@]+@[^\s@]+$/;

export function failureOf(error: unknown): InviteFailure {
  if (!(error instanceof ApiError)) {
    return 'failed';
  }
  if (error.status === 404 && error.code === 'user_not_found') {
    return 'unknown';
  }
  if (error.status === 429) {
    return 'limited';
  }
  if (error.status === 422) {
    const emailField = error.details.find((detail) => detail.field === 'email');
    if (emailField === undefined) {
      return 'failed';
    }
    return EMAIL_CODES.includes(emailField.code ?? '') ? 'email' : 'owner';
  }
  return error.status === 403 ? 'forbidden' : 'failed';
}

export function waitSeconds(error: unknown): number {
  return error instanceof ApiError && error.retryAfter !== null ? error.retryAfter : DEFAULT_WAIT_SECONDS;
}

export function InviteDialog({ endpointId, open, onOpenChange }: InviteDialogProps) {
  const returnFocus = useReturnFocus();
  const { t } = useTranslation();
  const baseId = useId();
  const put = usePutMember(endpointId);
  const [email, setEmail] = useState('');
  const [role, setRole] = useState<MemberRole>('viewer');
  const [failure, setFailure] = useState<InviteFailure | null>(null);
  const [wait, setWait] = useState(DEFAULT_WAIT_SECONDS);
  const emailId = `${baseId}-email`;
  const roleId = `${baseId}-role`;
  const errorId = `${baseId}-error`;

  function reset(): void {
    setEmail('');
    setRole('viewer');
    setFailure(null);
    put.reset();
  }

  function changeOpen(next: boolean): void {
    if (!next) {
      reset();
    }
    onOpenChange(next);
  }

  function submit(event: FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    const trimmed = email.trim();
    if (!EMAIL_SHAPE.test(trimmed)) {
      setFailure('email');
      return;
    }
    setFailure(null);
    put.mutate(
      { email: trimmed, role },
      {
        onSuccess: () => {
          changeOpen(false);
        },
        onError: (error) => {
          setWait(waitSeconds(error));
          setFailure(failureOf(error));
        },
      },
    );
  }

  function failureText(kind: InviteFailure): string {
    switch (kind) {
      case 'email':
        return t('wl.access.members.invite.error.email');
      case 'unknown':
        return t('wl.access.members.invite.error.unknown');
      case 'owner':
        return t('wl.access.members.invite.error.owner');
      case 'forbidden':
        return t('wl.access.members.error.forbidden');
      case 'limited':
        return t('wl.access.members.invite.error.limited', { seconds: wait });
      case 'failed':
        return t('wl.access.members.invite.error.failed');
    }
  }

  return (
    <Dialog.Root open={open} onOpenChange={changeOpen}>
      <Dialog.Portal>
        <Dialog.Overlay className={dialogStyles.overlay} />
        <Dialog.Content
          className={dialogStyles.content}
          onOpenAutoFocus={returnFocus.onOpenAutoFocus}
          onCloseAutoFocus={returnFocus.onCloseAutoFocus}
        >
          <Dialog.Title className={dialogStyles.title}>{t('wl.access.members.invite.title')}</Dialog.Title>
          <Dialog.Description className={dialogStyles.body}>
            {t('wl.access.members.invite.description')}
          </Dialog.Description>
          <form className={styles.fields} onSubmit={submit} noValidate>
            <div className={styles.field}>
              <label htmlFor={emailId} className={styles.label}>
                {t('wl.access.members.invite.email')}
              </label>
              <input
                id={emailId}
                type="email"
                className={styles.input}
                autoComplete="off"
                spellCheck={false}
                value={email}
                aria-invalid={failure === 'email' || failure === 'unknown' ? true : undefined}
                aria-describedby={failure === null ? undefined : errorId}
                onChange={(event) => {
                  setEmail(event.target.value);
                  setFailure(null);
                }}
              />
            </div>
            <div className={styles.field}>
              <label htmlFor={roleId} className={styles.label}>
                {t('wl.access.members.invite.role')}
              </label>
              <select
                id={roleId}
                className={styles.select}
                value={role}
                onChange={(event) => {
                  setRole(event.target.value === 'editor' ? 'editor' : 'viewer');
                }}
              >
                <option value="viewer">{t('wl.access.members.role.viewer')}</option>
                <option value="editor">{t('wl.access.members.role.editor')}</option>
              </select>
            </div>
            {failure === null ? null : (
              <p id={errorId} role="alert" className={styles.error}>
                {failureText(failure)}
              </p>
            )}
            <div className={dialogStyles.actions}>
              <Dialog.Close asChild>
                <button type="button" className={dialogStyles.secondary}>
                  {t('wl.access.members.invite.cancel')}
                </button>
              </Dialog.Close>
              <button type="submit" className={styles.primary} disabled={put.isPending}>
                {put.isPending ? t('wl.access.members.invite.sending') : t('wl.access.members.invite.send')}
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
