import { useLocation } from '@tanstack/react-router';
import { useInstance } from '../../api/queries';
import { StatePage } from '../../components/StatePage/StatePage';
import { signInHref } from '../../app/signIn';
import { UsagePage } from '../../features/usage/UsagePage';
import { UsageUnavailable } from '../../features/usage/UsageUnavailable';

export function UsageScreen() {
  const instance = useInstance().data;
  const location = useLocation();

  if (instance === undefined) {
    return null;
  }
  if (instance.auth_mode === 'open') {
    return <UsageUnavailable />;
  }
  if (instance.user === null) {
    return (
      <StatePage
        kind="signin"
        detail={{ providerName: instance.auth.provider_name, signInHref: signInHref(location.href) }}
      />
    );
  }
  return <UsagePage />;
}
