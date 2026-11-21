import { Link, useParams } from '@tanstack/react-router';
import { Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import { useViewportLayout } from '../../features/endpoint/useViewport';
import { findWorkspaceSection } from '../../features/workspace/sections';
import styles from './SettingsSectionScreen.module.css';

const SECTION_ROUTE_ID = '/shell/e/$id/settings/$section';

function SectionLoading() {
  const { t } = useTranslation();
  return (
    <p role="status" className={styles.loading}>
      {t('wl.workspace.loading')}
    </p>
  );
}

export function SettingsSectionScreen() {
  const { t } = useTranslation();
  const { id: endpointId, section } = useParams({ from: SECTION_ROUTE_ID });
  const layout = useViewportLayout();
  const Component = findWorkspaceSection(section)?.Component ?? null;

  return (
    <>
      {layout === 'narrow' ? (
        <Link to="/e/$id/settings" params={{ id: endpointId }} className={styles.back}>
          {t('wl.workspace.backToList')}
        </Link>
      ) : null}
      {Component === null ? (
        <p className={styles.comingSoon}>{t('wl.workspace.section.comingSoon')}</p>
      ) : (
        <Suspense fallback={<SectionLoading />}>
          <Component endpointId={endpointId} />
        </Suspense>
      )}
    </>
  );
}
