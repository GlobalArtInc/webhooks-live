import { endpointPath, requestJson } from '../../api/client';
import type { Endpoint } from '../../api/types';
import { clearGrant, storeGrant } from './grants';
import type { AccessPatch, UnlockGrant } from './types';

function isUnlockGrant(value: unknown): value is UnlockGrant {
  return (
    typeof value === 'object' &&
    value !== null &&
    'grant' in value &&
    'expires_at' in value &&
    typeof value.grant === 'string' &&
    typeof value.expires_at === 'string'
  );
}

export function patchAccess(endpointId: string, patch: AccessPatch): Promise<Endpoint> {
  return requestJson<Endpoint>(endpointPath(endpointId), { method: 'PATCH', body: patch });
}

export async function unlockEndpoint(endpointId: string, password: string): Promise<UnlockGrant> {
  const response = await requestJson<unknown>(`${endpointPath(endpointId)}/unlock`, {
    method: 'POST',
    body: { password },
  });
  if (!isUnlockGrant(response)) {
    throw new TypeError('The unlock answer is malformed.');
  }
  return response;
}

export async function adoptViewGrant(endpointId: string, saved: Endpoint, password: string): Promise<boolean> {
  const grant = saved.view_grant;
  if (grant !== undefined && grant !== null && storeGrant(endpointId, grant.grant, grant.expires_at)) {
    return true;
  }
  return renewGrant(endpointId, password);
}

export async function renewGrant(endpointId: string, password: string): Promise<boolean> {
  clearGrant(endpointId);
  try {
    const grant = await unlockEndpoint(endpointId, password);
    return storeGrant(endpointId, grant.grant, grant.expires_at);
  } catch {
    return false;
  }
}
