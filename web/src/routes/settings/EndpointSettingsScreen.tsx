import { Outlet, useParams } from '@tanstack/react-router';
import { useEndpoint } from '../../api/queries';
import { useViewportLayout } from '../../features/endpoint/useViewport';
import { BackToCaptures } from '../../features/workspace/BackToCaptures';
import { WorkspaceNav, type WorkspaceNavVariant } from '../../features/workspace/WorkspaceNav';
import styles from './EndpointSettingsScreen.module.css';

const ENDPOINT_ROUTE_ID = '/shell/e/$id';

const VARIANT_BY_LAYOUT: Record<'wide' | 'medium', WorkspaceNavVariant> = { wide: 'side', medium: 'tabs' };

export function EndpointSettingsScreen() {
  const { id: endpointId } = useParams({ from: ENDPOINT_ROUTE_ID });
  const endpoint = useEndpoint(endpointId).data;
  const layout = useViewportLayout();

  if (layout === 'narrow') {
    return (
      <div className={styles.narrow}>
        <Outlet />
      </div>
    );
  }

  const backLink = <BackToCaptures endpointId={endpointId} className={styles.back} />;

  if (layout === 'wide') {
    return (
      <div className={styles.wide}>
        <aside className={styles.aside}>
          {backLink}
          <WorkspaceNav endpointId={endpointId} endpoint={endpoint} variant={VARIANT_BY_LAYOUT.wide} />
        </aside>
        <div className={styles.content}>
          <Outlet />
        </div>
      </div>
    );
  }

  return (
    <div className={styles.medium}>
      <div className={styles.strip}>
        {backLink}
        <WorkspaceNav endpointId={endpointId} endpoint={endpoint} variant={VARIANT_BY_LAYOUT.medium} />
      </div>
      <div className={styles.content}>
        <Outlet />
      </div>
    </div>
  );
}
