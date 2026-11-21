import { setGrantRejectedHandler } from '../../api/client';
import { safeReturnPath } from '../../app/signIn';
import { clearGrant } from './grants';

export interface GuardLocation {
  pathname: string;
  href: string;
}

export interface GuardRouter {
  state: { status: string; location: GuardLocation };
  history: { replace: (path: string) => void };
}

const PENDING = 'pending';

export function unlockUrl(endpointId: string, next: string | null): string {
  const base = `/e/${endpointId}/unlock`;
  return next === null ? base : `${base}?next=${encodeURIComponent(next)}`;
}

export function installGrantGuard(router: GuardRouter): void {
  setGrantRejectedHandler((endpointId) => {
    clearGrant(endpointId);
    const { pathname, href } = router.state.location;
    if (router.state.status === PENDING || pathname === `/e/${endpointId}/unlock`) {
      return;
    }
    router.history.replace(unlockUrl(endpointId, safeReturnPath(href)));
  });
}
