import { useTranslation } from 'react-i18next';
import styles from './SectionNav.module.css';

export interface SectionNavItem {
  id: string;
  label: string;
}

export interface SectionNavProps {
  sections: readonly SectionNavItem[];
  activeId: string | null;
  onSelect: (id: string) => void;
}

export function SectionNav({ sections, activeId, onSelect }: SectionNavProps) {
  const { t } = useTranslation();
  return (
    <nav aria-label={t('wl.detail.nav.label')} className={styles.nav}>
      <ul className={styles.list}>
        {sections.map((section) => {
          const active = section.id === activeId;
          return (
            <li key={section.id}>
              <button
                type="button"
                className={active ? `${styles.link} ${styles.active}` : styles.link}
                aria-current={active ? 'location' : undefined}
                onClick={() => {
                  onSelect(section.id);
                }}
              >
                {section.label}
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
