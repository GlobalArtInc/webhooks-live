import { useId, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './CaptureDetail.module.css';
import type { DetailSectionId } from './types';

interface DetailSectionProps {
  id: DetailSectionId;
  title: string;
  collapsed: boolean;
  onToggle: (id: DetailSectionId) => void;
  children: ReactNode;
}

export function DetailSection({ id, title, collapsed, onToggle, children }: DetailSectionProps) {
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
          className={styles.collapse}
          aria-expanded={!collapsed}
          aria-controls={contentId}
          aria-label={
            collapsed ? t('wl.detail.section.showNamed', { title }) : t('wl.detail.section.hideNamed', { title })
          }
          onClick={() => {
            onToggle(id);
          }}
        >
          {collapsed ? t('wl.detail.section.show') : t('wl.detail.section.hide')}
        </button>
      </header>
      <div id={contentId} hidden={collapsed} className={styles.sectionBody}>
        {children}
      </div>
    </section>
  );
}
