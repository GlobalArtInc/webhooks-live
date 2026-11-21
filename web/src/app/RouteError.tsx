import { useLocation, useParams, useRouter, type ErrorComponentProps } from '@tanstack/react-router';
import { ApiError } from '../api/client';
import { useInstance } from '../api/queries';
import { RemoveFromMyEndpoints } from '../features/endpoints';
import { StatePage } from '../components/StatePage/StatePage';
import { stateKindForError } from '../components/StatePage/stateKind';
import { signInHref } from './signIn';
import { signOut } from './signOut';

export function RouteError({ error, reset }: ErrorComponentProps) {
  const router = useRouter();
  const location = useLocation();
  const instance = useInstance();
  const params = useParams({ strict: false });
  const status = error instanceof ApiError ? error.status : undefined;
  const kind = stateKindForError(error);
  const endpointId = typeof params.id === 'string' ? params.id : undefined;
  const removeAction =
    endpointId !== undefined && (kind === 'gone' || kind === 'not-found') ? (
      <RemoveFromMyEndpoints endpointId={endpointId} />
    ) : undefined;

  function retry() {
    void router.invalidate();
    reset();
  }

  return (
    <StatePage
      kind={kind}
      detail={{
        status,
        identity: instance.data?.user?.email,
        providerName: instance.data?.auth.provider_name,
        signInHref: signInHref(location.href),
      }}
      onRetry={retry}
      onSignOut={() => void signOut()}
      actions={removeAction}
    />
  );
}
