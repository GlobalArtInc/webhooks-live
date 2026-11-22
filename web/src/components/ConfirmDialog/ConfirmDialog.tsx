import * as Dialog from '@radix-ui/react-dialog';
import { useReturnFocus } from '../useReturnFocus';
import styles from './ConfirmDialog.module.css';

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  body: string;
  confirmLabel: string;
  cancelLabel: string;
  busy: boolean;
  onConfirm: () => void;
  onOpenChange: (open: boolean) => void;
  onCloseAutoFocus?: (event: Event) => void;
}

export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  cancelLabel,
  busy,
  onConfirm,
  onOpenChange,
  onCloseAutoFocus,
}: ConfirmDialogProps) {
  const returnFocus = useReturnFocus();
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content
          className={styles.content}
          onOpenAutoFocus={returnFocus.onOpenAutoFocus}
          onCloseAutoFocus={onCloseAutoFocus ?? returnFocus.onCloseAutoFocus}
        >
          <Dialog.Title className={styles.title}>{title}</Dialog.Title>
          <Dialog.Description className={styles.body}>{body}</Dialog.Description>
          <div className={styles.actions}>
            <Dialog.Close asChild>
              <button type="button" className={styles.secondary}>
                {cancelLabel}
              </button>
            </Dialog.Close>
            <button type="button" className={styles.danger} disabled={busy} onClick={onConfirm}>
              {confirmLabel}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
