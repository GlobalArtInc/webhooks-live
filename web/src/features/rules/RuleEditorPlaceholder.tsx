import { Link, useParams } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import styles from './RuleEditorPlaceholder.module.css';

export const RULE_EDITOR_ROUTE_ID = '/shell/e/$id/settings/rules/$ruleId';

export function RuleEditorPlaceholder() {
  const { t } = useTranslation();
  const { id: endpointId } = useParams({ from: RULE_EDITOR_ROUTE_ID });
  return (
    <section className={styles.page}>
      <h2 className={styles.title}>{t('wl.rules.editor.title')}</h2>
      <p className={styles.body}>{t('wl.rules.editor.arrives')}</p>
      <Link to="/e/$id/settings/$section" params={{ id: endpointId, section: 'rules' }} className={styles.back}>
        {t('wl.rules.editor.back')}
      </Link>
    </section>
  );
}
