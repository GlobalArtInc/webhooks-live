import { Navigate, useSearch } from '@tanstack/react-router';
import { useInstance } from '../../api/queries';
import { StatePage } from '../../components/StatePage/StatePage';
import { isAnonymousSso, signInHref } from '../../app/signIn';

export function SignInScreen() {
  const { next } = useSearch({ from: '/shell/signin' });
  const instance = useInstance();
  const target = next ?? '/';

  if (instance.data === undefined) {
    return null;
  }
  if (!isAnonymousSso(instance.data)) {
    return <Navigate to={target} />;
  }
  return (
    <StatePage
      kind="signin"
      detail={{ providerName: instance.data.auth.provider_name, signInHref: signInHref(target) }}
    />
  );
}
