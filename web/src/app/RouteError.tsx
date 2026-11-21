import { useLocation, useRouter, type ErrorComponentProps } from '@tanstack/react-router';
import { ApiError } from '../api/client';
import { useInstance } from '../api/queries';
import { StatePage } from '../components/StatePage/StatePage';
import { stateKindForError } from '../components/StatePage/stateKind';
import { signInHref } from './signIn';
import { signOut } from './signOut';

export function RouteError({ error, reset }: ErrorComponentProps) {
  const router = useRouter();
  const location = useLocation();
  const instance = useInstance();
  const status = error instanceof ApiError ? error.status : undefined;

  function retry() {
    void router.invalidate();
    reset();
  }

  return (
    <StatePage
      kind={stateKindForError(error)}
      detail={{
        status,
        identity: instance.data?.user?.email,
        providerName: instance.data?.auth.provider_name,
        signInHref: signInHref(location.href),
      }}
      onRetry={retry}
      onSignOut={() => void signOut()}
    />
  );
}
