import type { Instance } from '../../api/types';

export const PUBLIC_URL = 'https://webhooks.example.test';

export const instanceFixture: Instance = {
  version: '1.0.0',
  auth_mode: 'open',
  user: null,
  public_url: PUBLIC_URL,
  auth: { provider_name: null, sign_in_available: false },
  limits: {
    max_captures: 500,
    max_body_bytes: 10485760,
    retention_seconds: 604800,
    max_delay_ms: 10000,
    max_forward_rules: 5,
    max_response_body_bytes: 65536,
    max_response_headers: 20,
  },
  features: {
    ring_mode_allowed: true,
    forwarding: true,
    private_forward_targets_allowed: false,
  },
};
