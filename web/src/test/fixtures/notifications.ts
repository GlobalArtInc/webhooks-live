import type { Channel, EmailAddress, SecretSummary } from '../../features/notifications/types';

export const SLACK_CHANNEL_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4c01';
export const EMAIL_CHANNEL_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4c02';
export const VERIFIED_ADDRESS_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4d01';
export const PENDING_ADDRESS_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4d02';
export const VERIFIED_ADDRESS = 'alex@mail.example.test';
export const PENDING_ADDRESS = 'pending@mail.example.test';
export const VERIFY_TOKEN = 'test-only-verify-token';
export const DISABLE_TOKEN = 'test-only-disable-token';
export const WRONG_TOKEN = 'test-only-wrong-token';

const CREATED_AT = '2026-11-21T10:20:00.000Z';

export const slackChannelFixture: Channel = {
  id: SLACK_CHANNEL_ID,
  kind: 'slack',
  name: 'ops-chat',
  enabled: true,
  trigger: 'capture',
  condition: null,
  config: { url: 'https://hooks.slack.com/services/...' },
  template: null,
  per_hour: 60,
  created_at: CREATED_AT,
};

export const emailChannelFixture: Channel = {
  id: EMAIL_CHANNEL_ID,
  kind: 'email',
  name: 'alex mail',
  enabled: false,
  trigger: 'capture',
  condition: null,
  config: { address_id: VERIFIED_ADDRESS_ID },
  template: null,
  per_hour: 5,
  created_at: CREATED_AT,
};

export const verifiedAddressFixture: EmailAddress = {
  id: VERIFIED_ADDRESS_ID,
  address: VERIFIED_ADDRESS,
  verified: true,
  created_at: CREATED_AT,
};

export const pendingAddressFixture: EmailAddress = {
  id: PENDING_ADDRESS_ID,
  address: PENDING_ADDRESS,
  verified: false,
  created_at: CREATED_AT,
};

export const secretFixtures: SecretSummary[] = [
  { name: 'TELEGRAM_TOKEN', created_at: CREATED_AT, updated_at: CREATED_AT },
];
