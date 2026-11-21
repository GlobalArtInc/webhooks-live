import { redirect } from '@tanstack/react-router';
import { safeReturnPath } from '../../app/signIn';
import { isGrantRequired } from './grantRequired';

export async function ensureUnlocked<T>(load: () => Promise<T>, endpointId: string, returnPath: string): Promise<T> {
  try {
    return await load();
  } catch (failure: unknown) {
    if (isGrantRequired(failure)) {
      throw redirect({
        to: '/e/$id/unlock',
        params: { id: endpointId },
        search: { next: safeReturnPath(returnPath) ?? undefined },
        replace: true,
      });
    }
    throw failure;
  }
}
