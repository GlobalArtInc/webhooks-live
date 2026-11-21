import { useTranslation } from 'react-i18next';
import { CreateForm, MyEndpointsList } from '../../features/endpoints';
import styles from './HomeScreen.module.css';

export function HomeScreen() {
  const { t } = useTranslation();

  return (
    <section className={styles.page}>
      <div className={styles.hero}>
        <h1 className={styles.title}>{t('wl.home.hero.title')}</h1>
        <p className={styles.pitch}>{t('wl.home.hero.pitch')}</p>
        <CreateForm />
      </div>
      <MyEndpointsList />
    </section>
  );
}
