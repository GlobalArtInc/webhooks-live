import { Outlet, useLocation, useNavigate } from '@tanstack/react-router';
import type { ReactNode } from 'react';
import { useInstance } from '../../api/queries';
import { AccountControls } from '../../features/account';
import { EndpointRail } from '../../features/endpoints';
import { useShortcutHandlers } from '../../features/shortcuts';
import { TokensLink } from '../../features/tokens/TokensLink';
import { UsageLink } from '../../features/usage/UsageLink';
import { StatePage } from '../../components/StatePage/StatePage';
import { requiresSignIn, signInHref } from '../signIn';
import { useDocumentTitle } from '../useDocumentTitle';
import { AppShell } from './AppShell';
import { collapsesRail, fillsViewport, showsEndpointRail } from './rail';

export function ShellLayout() {
  const instance = useInstance();
  const location = useLocation();
  const navigate = useNavigate();
  useDocumentTitle(location.pathname);

  useShortcutHandlers({
    newEndpoint: () => {
      void navigate({ to: '/' });
    },
  });

  let content: ReactNode;
  let rail: ReactNode;
  let header: ReactNode;
  if (instance.isError) {
    content = <StatePage kind="error" onRetry={() => void instance.refetch()} />;
  } else if (instance.data === undefined) {
    content = null;
  } else if (requiresSignIn(instance.data, location.pathname)) {
    content = (
      <StatePage
        kind="signin"
        detail={{ providerName: instance.data.auth.provider_name, signInHref: signInHref(location.href) }}
      />
    );
  } else {
    content = <Outlet />;
    header = (
      <>
        <TokensLink instance={instance.data} />
        <UsageLink instance={instance.data} />
        <AccountControls instance={instance.data} />
      </>
    );
    rail = showsEndpointRail(location.pathname) ? <EndpointRail /> : undefined;
  }

  return (
    <AppShell
      resetKey={location.href}
      header={header}
      rail={rail}
      collapseRail={collapsesRail(location.pathname)}
      fill={fillsViewport(location.pathname)}
    >
      {content}
    </AppShell>
  );
}
