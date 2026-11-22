import type { Instance } from './types';

export function isSignedIn(instance: Instance | undefined): boolean {
  return instance !== undefined && instance.user !== null;
}

export function canSignIn(instance: Instance | undefined): boolean {
  return instance !== undefined && instance.auth.sign_in_available;
}

export function offersUsage(instance: Instance | undefined): boolean {
  return instance !== undefined && instance.auth_mode !== 'open' && instance.user !== null;
}

export function offersAliases(instance: Instance | undefined): boolean {
  return isSignedIn(instance) && instance?.limits.aliases !== 0;
}

export function offersImport(instance: Instance | undefined): boolean {
  return instance !== undefined && instance.limits.import_max_bytes !== 0;
}

export function offersSecrets(instance: Instance | undefined): boolean {
  return instance !== undefined && instance.features.secrets !== false;
}

export function offersNotifications(instance: Instance | undefined): boolean {
  return (instance?.features.notifications ?? []).length > 0;
}

export function offersSendHistory(instance: Instance | undefined, endpointId: string | null): boolean {
  return endpointId !== null || isSignedIn(instance);
}
