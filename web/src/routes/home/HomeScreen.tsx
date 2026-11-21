import { useNavigate } from '@tanstack/react-router';
import { lazy, Suspense, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CreateForm, MyEndpointsList } from '../../features/endpoints';
import styles from './HomeScreen.module.css';

const ImportDialog = lazy(() =>
  import('../../features/importdialog/ImportDialog').then((module) => ({ default: module.ImportDialog })),
);

export function HomeScreen() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [importOpen, setImportOpen] = useState(false);

  return (
    <section className={styles.page}>
      <div className={styles.hero}>
        <h1 className={styles.title}>{t('wl.home.hero.title')}</h1>
        <p className={styles.pitch}>{t('wl.home.hero.pitch')}</p>
        <CreateForm />
        <button
          type="button"
          className={styles.importButton}
          onClick={() => {
            setImportOpen(true);
          }}
        >
          {t('wl.importdialog.open')}
        </button>
      </div>
      <MyEndpointsList />
      {importOpen ? (
        <Suspense fallback={null}>
          <ImportDialog
            endpointId={null}
            onOpenChange={setImportOpen}
            onOpenEndpoint={(openId) => {
              setImportOpen(false);
              void navigate({ to: '/e/$id', params: { id: openId } });
            }}
          />
        </Suspense>
      ) : null}
    </section>
  );
}
