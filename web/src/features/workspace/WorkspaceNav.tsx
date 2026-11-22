import { Link, useParams } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import type { Endpoint } from '../../api/types';
import { useScrollCue } from '../../components/ScrollCue/useScrollCue';
import cueStyles from '../../components/ScrollCue/ScrollCue.module.css';
import { WORKSPACE_SECTIONS, type WorkspaceSection } from './sections';
import styles from './WorkspaceNav.module.css';

export type WorkspaceNavVariant = 'side' | 'tabs' | 'list';

export interface WorkspaceNavProps {
  endpointId: string;
  endpoint: Endpoint | undefined;
  variant: WorkspaceNavVariant;
}

function CountBadge({ section, endpoint }: { section: WorkspaceSection; endpoint: Endpoint | undefined }) {
  if (section.count === undefined || endpoint === undefined) {
    return null;
  }
  const count = section.count(endpoint);
  return count > 0 ? <span className={styles.count}>{count}</span> : null;
}

export function WorkspaceNav({ endpointId, endpoint, variant }: WorkspaceNavProps) {
  const { t } = useTranslation();
  const sections = WORKSPACE_SECTIONS.filter((section) => section.listed !== false);
  const activeId = useParams({ strict: false }).section ?? null;
  const { ref, edges } = useScrollCue<HTMLUListElement>(activeId);
  const scrolling = variant === 'tabs';
  return (
    <nav
      aria-label={t('wl.workspace.nav.label')}
      className={scrolling ? `${styles[variant]} ${cueStyles.cue}` : styles[variant]}
      data-fade-start={scrolling ? edges.start : undefined}
      data-fade-end={scrolling ? edges.end : undefined}
    >
      <ul ref={ref} className={styles.list}>
        {sections.map((section) => {
          const label = t(section.labelKey);
          if (section.Component === null) {
            return (
              <li key={section.id}>
                <span className={styles.disabled} aria-disabled="true">
                  {label}
                  <CountBadge section={section} endpoint={endpoint} />
                </span>
              </li>
            );
          }
          return (
            <li key={section.id}>
              <Link
                to="/e/$id/settings/$section"
                params={{ id: endpointId, section: section.id }}
                className={styles.link}
                activeProps={{ className: `${styles.link} ${styles.active}` }}
              >
                {label}
                <CountBadge section={section} endpoint={endpoint} />
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
