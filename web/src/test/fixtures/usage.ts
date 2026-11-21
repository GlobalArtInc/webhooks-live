import type { Instance, User } from '../../api/types';
import type { Usage, UsageMeter } from '../../api/usage';
import { instanceFixture } from './instance';

const MINUTE_MS = 60_000;
const BYTES_PER_GIGABYTE = 1_000_000_000;

export const SIGNED_IN_USER: User = {
  id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21',
  email: 'owner@example.test',
  name: 'Owner',
};

export const SIGNED_IN_INSTANCE: Instance = { ...instanceFixture, auth_mode: 'mixed', user: SIGNED_IN_USER };

export function resetsSoon(): string {
  return new Date(Date.now() + 14 * MINUTE_MS).toISOString();
}

export function meterFixture(overrides: Partial<UsageMeter> = {}): UsageMeter {
  return { name: 'endpoints', used: 7, max: 20, window: 'total', resets_at: null, ...overrides };
}

export function usageFixture(items: UsageMeter[] = defaultMeters()): Usage {
  return { tier: 'member', items };
}

export function defaultMeters(): UsageMeter[] {
  return [
    meterFixture(),
    meterFixture({ name: 'storage_bytes', used: 18_200_000, max: BYTES_PER_GIGABYTE, window: 'total' }),
    meterFixture({ name: 'outbound', used: 12, max: 200, window: 'hour', resets_at: resetsSoon() }),
  ];
}

export function nearStorageMeter(): UsageMeter {
  return meterFixture({ name: 'storage_bytes', used: 900_000_000, max: BYTES_PER_GIGABYTE, window: 'total' });
}

export function fullOutboundMeter(): UsageMeter {
  return meterFixture({ name: 'outbound', used: 200, max: 200, window: 'hour', resets_at: resetsSoon() });
}
