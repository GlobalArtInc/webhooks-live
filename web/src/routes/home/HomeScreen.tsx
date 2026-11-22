import { useNavigate } from '@tanstack/react-router';
import { lazy, Suspense, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { offersImport } from '../../api/capabilities';
import { useInstance } from '../../api/queries';
import { CreateForm, MyEndpointsList } from '../../features/endpoints';
import { CurlExample } from './CurlExample';
import { TrySteps } from './TrySteps';
import styles from './HomeScreen.module.css';

const ImportDialog = lazy(() =>
  import('../../features/importdialog/ImportDialog').then((module) => ({ default: module.ImportDialog })),
);

export function HomeScreen() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [importOpen, setImportOpen] = useState(false);
  const importOffered = offersImport(useInstance().data);

  return (
    <section className={styles.page}>
      <div className={styles.content}>
        <div className={styles.hero}>
          <h1 className={styles.title}>{t('wl.home.hero.title')}</h1>
          <p className={styles.lede}>{t('wl.home.hero.pitch')}</p>
          <CreateForm />
        </div>
        <div className={styles.demo}>
          <TrySteps />
          <CurlExample />
          {importOffered ? (
            <button
              type="button"
              className={styles.importButton}
              onClick={() => {
                setImportOpen(true);
              }}
            >
              {t('wl.importdialog.open')}
            </button>
          ) : null}
        </div>
        <MyEndpointsList />
      </div>
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
