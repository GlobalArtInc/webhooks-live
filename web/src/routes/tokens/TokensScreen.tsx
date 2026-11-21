import { useLocation } from '@tanstack/react-router';
import { useInstance } from '../../api/queries';
import { StatePage } from '../../components/StatePage/StatePage';
import { signInHref } from '../../app/signIn';
import { TokensPage, TokensUnavailable } from '../../features/tokens';

export function TokensScreen() {
  const instance = useInstance().data;
  const location = useLocation();

  if (instance === undefined) {
    return null;
  }
  if (instance.auth_mode === 'open') {
    return <TokensUnavailable />;
  }
  if (instance.user === null) {
    return (
      <StatePage
        kind="signin"
        detail={{ providerName: instance.auth.provider_name, signInHref: signInHref(location.href) }}
      />
    );
  }
  return <TokensPage returnTo={location.href} providerName={instance.auth.provider_name} />;
}
