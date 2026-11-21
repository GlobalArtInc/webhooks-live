import type { Instance } from '../../api/types';

export const PUBLIC_URL = 'https://webhooks.example.test';

export const instanceFixture: Instance = {
  version: '1.0.0',
  auth_mode: 'open',
  user: null,
  public_url: PUBLIC_URL,
  limits: {
    max_captures: 500,
    max_body_bytes: 10485760,
    retention_seconds: 604800,
    max_delay_ms: 10000,
    max_forward_rules: 5,
  },
  features: { forwarding: true },
};
