import type { UsageReport } from '../../api/general';

export const EXAMPLE_SECRET_NAME = 'example-signing-secret';

export const usageFixture: UsageReport = {
  meters: [
    { name: 'endpoints', used: 7, max: 20, window: 'total', resets_at: null },
    { name: 'storage_bytes', used: 43000000, max: 1073741824, window: 'total', resets_at: null },
    { name: 'aliases', used: 2, max: 5, window: 'total', resets_at: null },
    { name: 'outbound', used: 12, max: 100, window: 'hour', resets_at: '2026-11-21T15:00:00.000Z' },
  ],
};
