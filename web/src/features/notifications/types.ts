import type { Timestamp, Uuid } from '../../api/types';

export const CHANNEL_KINDS = ['webhook', 'slack', 'discord', 'telegram', 'email', 'push'] as const;

export type ChannelKind = (typeof CHANNEL_KINDS)[number];

export type ChannelTrigger = 'capture' | 'rule';

export interface ChannelConfig {
  url?: string;
  token_secret?: string;
  chat_id?: string;
  address_id?: string;
}

export interface Channel {
  id: Uuid;
  kind: ChannelKind;
  name: string;
  enabled: boolean;
  trigger: ChannelTrigger;
  condition: unknown;
  config: ChannelConfig;
  template: string | null;
  per_hour: number;
  created_at: Timestamp;
}

export interface ChannelList {
  items: Channel[];
}

export interface ChannelInput {
  kind?: ChannelKind;
  name?: string;
  enabled?: boolean;
  trigger?: 'capture';
  per_hour?: number;
  config?: ChannelConfig;
}

export type DeliveryState = 'pending' | 'running' | 'succeeded' | 'failed' | 'suppressed';

export interface Delivery {
  id: Uuid;
  channel_id: Uuid;
  kind: 'single' | 'summary' | 'test';
  state: DeliveryState;
  attempts: number;
  status: number | null;
  error: string | null;
  created_at: Timestamp;
  finished_at: Timestamp | null;
}

export interface EmailAddress {
  id: Uuid;
  address: string;
  verified: boolean;
  created_at: Timestamp;
}

export interface EmailAddressList {
  items: EmailAddress[];
}

export interface SecretSummary {
  name: string;
  created_at: Timestamp;
  updated_at: Timestamp;
}

export interface SecretList {
  items: SecretSummary[];
}

export interface PushPublicKey {
  public_key: string;
}

export interface PushKeys {
  p256dh: string;
  auth: string;
}

export interface PushSubscriptionInput {
  endpoint: string;
  keys: PushKeys;
  user_agent?: string;
}

export interface PushSubscribed {
  id: Uuid;
  created_at: Timestamp;
}
