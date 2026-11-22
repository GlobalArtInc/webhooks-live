import { useTranslation } from 'react-i18next';
import { offersSecrets } from '../../../api/capabilities';
import { useInstance } from '../../../api/queries';
import type { SectionProps } from '../sections';
import styles from './SecretsSection.module.css';

export function SecretsSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const instance = useInstance().data;
  const heading = <h2 className={styles.heading}>{t('wl.workspace.section.secrets')}</h2>;

  if (instance === undefined) {
    return (
      <div className={styles.page}>
        {heading}
        <p role="status" className={styles.note}>
          {t('wl.workspace.loading')}
        </p>
      </div>
    );
  }

  return (
    <div className={styles.page}>
      {heading}
      <p className={styles.note}>
        {offersSecrets(instance)
          ? t('wl.workspace.secrets.apiOnly', { path: `/api/v1/endpoints/${endpointId}/secrets` })
          : t('wl.workspace.secrets.unavailable')}
      </p>
    </div>
  );
}
