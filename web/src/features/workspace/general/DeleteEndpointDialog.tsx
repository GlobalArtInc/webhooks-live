import * as Dialog from '@radix-ui/react-dialog';
import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useDeleteEndpoint } from '../../../api/queries';
import { useReturnFocus } from '../../../components/useReturnFocus';
import { forgetEndpoint } from '../../endpoints';
import styles from './DeleteEndpointDialog.module.css';

export interface DeleteEndpointDialogProps {
  endpointId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDeleted: () => void;
}

export function DeleteEndpointDialog({ endpointId, open, onOpenChange, onDeleted }: DeleteEndpointDialogProps) {
  const returnFocus = useReturnFocus();
  const { t } = useTranslation();
  const inputId = useId();
  const remove = useDeleteEndpoint(endpointId);
  const [typed, setTyped] = useState('');
  const [failed, setFailed] = useState(false);
  const confirmed = typed === endpointId;

  function confirmDelete(): void {
    setFailed(false);
    remove.mutate(undefined, {
      onSuccess: () => {
        forgetEndpoint(endpointId);
        onDeleted();
      },
      onError: () => {
        setFailed(true);
      },
    });
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          setTyped('');
          setFailed(false);
        }
        onOpenChange(next);
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content
          className={styles.content}
          onOpenAutoFocus={returnFocus.onOpenAutoFocus}
          onCloseAutoFocus={returnFocus.onCloseAutoFocus}
        >
          <Dialog.Title className={styles.title}>{t('wl.general.danger.deleteEndpoint.title')}</Dialog.Title>
          <Dialog.Description className={styles.body}>{t('wl.general.danger.deleteEndpoint.body')}</Dialog.Description>
          <div className={styles.field}>
            <label htmlFor={inputId} className={styles.label}>
              {t('wl.general.danger.deleteEndpoint.typeLabel', { id: endpointId })}
            </label>
            <input
              id={inputId}
              type="text"
              autoComplete="off"
              spellCheck={false}
              className={styles.input}
              value={typed}
              onChange={(event) => {
                setTyped(event.target.value);
              }}
            />
          </div>
          {failed ? (
            <p role="alert" className={styles.error}>
              {t('wl.general.danger.deleteEndpoint.failed')}
            </p>
          ) : null}
          <div className={styles.actions}>
            <Dialog.Close asChild>
              <button type="button" className={styles.secondary}>
                {t('wl.general.danger.deleteEndpoint.cancel')}
              </button>
            </Dialog.Close>
            <button
              type="button"
              className={styles.danger}
              disabled={!confirmed || remove.isPending}
              onClick={confirmDelete}
            >
              {t('wl.general.danger.deleteEndpoint.confirm')}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
