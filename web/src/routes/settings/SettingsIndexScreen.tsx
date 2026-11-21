import { Navigate, useParams } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { useEndpoint } from '../../api/queries';
import { useViewportLayout } from '../../features/endpoint/useViewport';
import { BackToCaptures } from '../../features/workspace/BackToCaptures';
import { WorkspaceNav } from '../../features/workspace/WorkspaceNav';
import styles from './SettingsSectionScreen.module.css';

const ENDPOINT_ROUTE_ID = '/shell/e/$id';
const DEFAULT_SECTION = 'general';

export function SettingsIndexScreen() {
  const { t } = useTranslation();
  const { id: endpointId } = useParams({ from: ENDPOINT_ROUTE_ID });
  const endpoint = useEndpoint(endpointId).data;
  const layout = useViewportLayout();

  if (layout !== 'narrow') {
    return <Navigate to="/e/$id/settings/$section" params={{ id: endpointId, section: DEFAULT_SECTION }} replace />;
  }

  return (
    <div className={styles.list}>
      <BackToCaptures endpointId={endpointId} className={styles.back} />
      <h2 className={styles.heading}>{t('wl.workspace.configure')}</h2>
      <WorkspaceNav endpointId={endpointId} endpoint={endpoint} variant="list" />
    </div>
  );
}
