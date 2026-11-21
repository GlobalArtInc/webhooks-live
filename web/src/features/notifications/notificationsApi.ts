import { endpointPath, requestEmpty, requestJson } from '../../api/client';
import type {
  Channel,
  ChannelInput,
  ChannelList,
  Delivery,
  EmailAddress,
  EmailAddressList,
  PushPublicKey,
  PushSubscribed,
  PushSubscriptionInput,
  SecretList,
} from './types';

const EMAIL_ADDRESSES = '/email-addresses';
const PUSH_KEY = '/push/public-key';
const PUSH_SUBSCRIPTIONS = '/push/subscriptions';
const DISABLE = '/notifications/disable';
const EMAIL_VERIFY = '/email-addresses/verify';

function channelsPath(endpointId: string): string {
  return `${endpointPath(endpointId)}/notifications`;
}

function channelPath(endpointId: string, channelId: string): string {
  return `${channelsPath(endpointId)}/${encodeURIComponent(channelId)}`;
}

export function listChannels(endpointId: string): Promise<ChannelList> {
  return requestJson<ChannelList>(channelsPath(endpointId), { method: 'GET' });
}

export function createChannel(endpointId: string, input: ChannelInput): Promise<Channel> {
  return requestJson<Channel>(channelsPath(endpointId), { method: 'POST', body: input });
}

export function patchChannel(endpointId: string, channelId: string, input: ChannelInput): Promise<Channel> {
  return requestJson<Channel>(channelPath(endpointId, channelId), { method: 'PATCH', body: input });
}

export function deleteChannel(endpointId: string, channelId: string): Promise<void> {
  return requestEmpty(channelPath(endpointId, channelId), { method: 'DELETE' });
}

export function testChannel(endpointId: string, channelId: string): Promise<Delivery> {
  return requestJson<Delivery>(`${channelPath(endpointId, channelId)}/test`, { method: 'POST' });
}

export function listSecrets(endpointId: string): Promise<SecretList> {
  return requestJson<SecretList>(`${endpointPath(endpointId)}/secrets`, { method: 'GET' });
}

export function listEmailAddresses(): Promise<EmailAddressList> {
  return requestJson<EmailAddressList>(EMAIL_ADDRESSES, { method: 'GET' });
}

export function addEmailAddress(address: string): Promise<EmailAddress> {
  return requestJson<EmailAddress>(EMAIL_ADDRESSES, { method: 'POST', body: { address } });
}

export function deleteEmailAddress(addressId: string): Promise<void> {
  return requestEmpty(`${EMAIL_ADDRESSES}/${encodeURIComponent(addressId)}`, { method: 'DELETE' });
}

export function verifyEmailAddress(token: string): Promise<EmailAddress> {
  return requestJson<EmailAddress>(EMAIL_VERIFY, { method: 'POST', body: { token } });
}

export function disableNotifications(token: string): Promise<void> {
  return requestEmpty(DISABLE, { method: 'POST', body: { token } });
}

export function getPushPublicKey(): Promise<PushPublicKey> {
  return requestJson<PushPublicKey>(PUSH_KEY, { method: 'GET' });
}

export function subscribePush(input: PushSubscriptionInput): Promise<PushSubscribed> {
  return requestJson<PushSubscribed>(PUSH_SUBSCRIPTIONS, { method: 'POST', body: input });
}
