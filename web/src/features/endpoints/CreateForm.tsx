import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CreateErrorNotice } from './CreateErrorNotice';
import { ResponseForm } from './ResponseForm';
import { useCreateAndOpen } from './useCreateAndOpen';
import styles from './CreateForm.module.css';

export function CreateForm() {
  const { t } = useTranslation();
  const { create, isPending, error } = useCreateAndOpen();
  const [customOpen, setCustomOpen] = useState(false);
  const hintId = useId();
  const panelId = useId();

  return (
    <div className={styles.create}>
      <div className={styles.primaryRow}>
        <button
          type="button"
          className={styles.primary}
          aria-describedby={hintId}
          disabled={isPending}
          onClick={() => create({})}
        >
          {isPending ? t('wl.home.create.busy') : t('wl.home.create.button')}
        </button>
        <span id={hintId} className={styles.hint}>
          {t('wl.home.create.hint')}
        </span>
      </div>
      {error !== null && !isPending && <CreateErrorNotice error={error} onRetry={() => create({})} />}
      <button
        type="button"
        className={styles.disclosure}
        aria-expanded={customOpen}
        aria-controls={panelId}
        onClick={() => setCustomOpen((open) => !open)}
      >
        {t('wl.home.custom.toggle')}
      </button>
      <div id={panelId} hidden={!customOpen} className={styles.panel}>
        <ResponseForm />
      </div>
    </div>
  );
}
