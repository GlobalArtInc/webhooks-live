import { useNavigate } from '@tanstack/react-router';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useClearCaptures } from '../../../api/queries';
import type { EndpointRole } from '../../../api/types';
import { ConfirmDialog } from '../../../components/ConfirmDialog/ConfirmDialog';
import { DeleteEndpointDialog } from './DeleteEndpointDialog';
import styles from './DangerZone.module.css';

export interface DangerZoneProps {
  endpointId: string;
  captureCount: number;
  myRole: EndpointRole | null;
}

export function DangerZone({ endpointId, captureCount, myRole }: DangerZoneProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const clear = useClearCaptures(endpointId);
  const [clearOpen, setClearOpen] = useState(false);
  const [clearFailed, setClearFailed] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const canEdit = myRole !== 'viewer';
  const canDeleteEndpoint = myRole === null || myRole === 'owner';

  function clearAll(): void {
    setClearFailed(false);
    clear.mutate(undefined, {
      onSuccess: () => {
        setClearOpen(false);
      },
      onError: () => {
        setClearFailed(true);
      },
    });
  }

  return (
    <section className={styles.zone} aria-labelledby={`${endpointId}-danger`}>
      <h3 id={`${endpointId}-danger`} className={styles.heading}>
        {t('wl.general.danger.title')}
      </h3>
      <div className={styles.row}>
        <button
          type="button"
          className={styles.button}
          disabled={!canEdit || captureCount === 0}
          onClick={() => {
            setClearOpen(true);
          }}
        >
          {t('wl.general.danger.deleteAll')}
        </button>
        <button
          type="button"
          className={styles.danger}
          disabled={!canDeleteEndpoint}
          onClick={() => {
            setDeleteOpen(true);
          }}
        >
          {t('wl.general.danger.delete')}
        </button>
      </div>
      {canDeleteEndpoint ? null : <p className={styles.note}>{t('wl.general.danger.forbidden')}</p>}
      {clearFailed ? (
        <p role="alert" className={styles.error}>
          {t('wl.endpoint.clear.failed')}
        </p>
      ) : null}
      <ConfirmDialog
        open={clearOpen}
        title={t('wl.endpoint.clear.title')}
        body={t('wl.endpoint.clear.body', { count: captureCount })}
        confirmLabel={t('wl.endpoint.clear.confirm')}
        cancelLabel={t('wl.endpoint.clear.cancel')}
        busy={clear.isPending}
        onConfirm={clearAll}
        onOpenChange={setClearOpen}
      />
      <DeleteEndpointDialog
        endpointId={endpointId}
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        onDeleted={() => {
          void navigate({ to: '/' });
        }}
      />
    </section>
  );
}
