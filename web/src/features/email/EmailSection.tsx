import { useId, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './EmailDetail.module.css';
import type { EmailSectionId } from './sections';

export interface EmailSectionProps {
  id: EmailSectionId;
  title: string;
  collapsed: boolean;
  onToggle: (id: EmailSectionId) => void;
  children: ReactNode;
}

export function EmailSection({ id, title, collapsed, onToggle, children }: EmailSectionProps) {
  const { t } = useTranslation();
  const headingId = useId();
  const contentId = useId();
  return (
    <section aria-labelledby={headingId} data-section={id} className={styles.section}>
      <header className={styles.sectionHeader}>
        <h2 id={headingId} tabIndex={-1} className={styles.sectionTitle}>
          {title}
        </h2>
        <button
          type="button"
          className={styles.action}
          aria-expanded={!collapsed}
          aria-controls={contentId}
          aria-label={
            collapsed ? t('wl.email.section.showNamed', { title }) : t('wl.email.section.hideNamed', { title })
          }
          onClick={() => {
            onToggle(id);
          }}
        >
          {collapsed ? t('wl.email.section.show') : t('wl.email.section.hide')}
        </button>
      </header>
      <div id={contentId} hidden={collapsed} className={styles.sectionBody}>
        {collapsed ? null : children}
      </div>
    </section>
  );
}
