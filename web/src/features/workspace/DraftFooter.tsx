import { useTranslation } from 'react-i18next';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { SaveBar } from '../../components/SaveBar/SaveBar';
import type { DraftStatus } from './useSettingsDraft';

export interface DraftFooterProps {
  status: DraftStatus;
  readOnly: boolean;
  saveBlockedText?: string;
}

export function DraftFooter({ status, readOnly, saveBlockedText }: DraftFooterProps) {
  const { t } = useTranslation();
  const { blocker } = status;
  return (
    <>
      {readOnly ? null : (
        <SaveBar
          dirty={status.dirty}
          saving={status.saving}
          failed={status.error !== null}
          blockedText={saveBlockedText}
          onSave={() => {
            void status.save();
          }}
          onDiscard={status.discard}
        />
      )}
      <ConfirmDialog
        open={blocker.status === 'blocked'}
        title={t('wl.workspace.guard.title')}
        body={t('wl.workspace.guard.body')}
        confirmLabel={t('wl.workspace.guard.discard')}
        cancelLabel={t('wl.workspace.guard.keep')}
        busy={false}
        onConfirm={() => {
          blocker.proceed?.();
        }}
        onOpenChange={(open) => {
          if (!open) {
            blocker.reset?.();
          }
        }}
      />
    </>
  );
}
