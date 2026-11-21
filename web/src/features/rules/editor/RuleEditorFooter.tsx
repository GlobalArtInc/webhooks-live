import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ConfirmDialog } from '../../../components/ConfirmDialog/ConfirmDialog';
import { SaveBar } from '../../../components/SaveBar/SaveBar';
import type { DirtyBlocker } from './useDirtyGuard';

export interface RuleEditorFooterProps {
  readOnly: boolean;
  dirty: boolean;
  saving: boolean;
  failed: boolean;
  blocker: DirtyBlocker;
  onSave: () => void;
  onDiscard: () => void;
}

export function RuleEditorFooter({
  readOnly,
  dirty,
  saving,
  failed,
  blocker,
  onSave,
  onDiscard,
}: RuleEditorFooterProps) {
  const { t } = useTranslation();
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  return (
    <>
      {readOnly ? null : (
        <SaveBar
          dirty={dirty}
          saving={saving}
          failed={failed}
          onSave={onSave}
          onDiscard={() => {
            setConfirmDiscard(true);
          }}
        />
      )}
      <ConfirmDialog
        open={confirmDiscard}
        title={t('wl.ruleeditor.discard.title')}
        body={t('wl.ruleeditor.discard.body')}
        confirmLabel={t('wl.ruleeditor.discard.confirm')}
        cancelLabel={t('wl.ruleeditor.discard.cancel')}
        busy={false}
        onConfirm={() => {
          setConfirmDiscard(false);
          onDiscard();
        }}
        onOpenChange={setConfirmDiscard}
      />
      <ConfirmDialog
        open={blocker.status === 'blocked'}
        title={t('wl.workspace.guard.title')}
        body={t('wl.ruleeditor.guard.body')}
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
