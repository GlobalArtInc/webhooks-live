import { Outlet, useLocation } from '@tanstack/react-router';
import type { ReactNode } from 'react';
import { useInstance } from '../../api/queries';
import { StatePage } from '../../components/StatePage/StatePage';
import { requiresSignIn, signInHref } from '../signIn';
import { AppShell } from './AppShell';

export function ShellLayout() {
  const instance = useInstance();
  const location = useLocation();

  let content: ReactNode;
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
  }

  return <AppShell resetKey={location.href}>{content}</AppShell>;
}
