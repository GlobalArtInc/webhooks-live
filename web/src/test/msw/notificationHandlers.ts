import { http, type HttpHandler } from 'msw';
import type { ChannelInput, Channel, Delivery, EmailAddress, SecretSummary } from '../../features/notifications/types';
import { DISABLE_TOKEN, VERIFY_TOKEN } from '../fixtures/notifications';
import { emptyResponse, errorResponse, jsonResponse, readJsonBody } from './responses';

export type TestStatus = 'succeeded' | 'failed' | 'rate-limited';

export interface NotificationApiState {
  channels: Channel[];
  addresses: EmailAddress[];
  secrets: SecretSummary[];
  testStatus: TestStatus;
  patches: ChannelInput[];
}

const CHANNELS = '/api/v1/endpoints/:endpointId/notifications';
const CHANNEL = '/api/v1/endpoints/:endpointId/notifications/:channelId';
const CHANNEL_TEST = '/api/v1/endpoints/:endpointId/notifications/:channelId/test';
const SECRETS = '/api/v1/endpoints/:endpointId/secrets';
const ADDRESSES = '/api/v1/email-addresses';
const ADDRESS = '/api/v1/email-addresses/:addressId';
const VERIFY = '/api/v1/email-addresses/verify';
const DISABLE = '/api/v1/notifications/disable';
const CREATED_AT = '2026-11-21T10:30:00.000Z';
const NEW_ID_PREFIX = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4e';

interface ChannelPath {
  channelId: string;
}

interface AddressPath {
  addressId: string;
}

function deliveryFor(channelId: string, testStatus: TestStatus): Delivery {
  const failed = testStatus === 'failed';
  return {
    id: `${NEW_ID_PREFIX}01`,
    channel_id: channelId,
    kind: 'test',
    state: failed ? 'failed' : 'succeeded',
    attempts: 1,
    status: failed ? 401 : 200,
    error: failed ? 'Unauthorized' : null,
    created_at: CREATED_AT,
    finished_at: CREATED_AT,
  };
}

function channelFrom(input: ChannelInput, id: string): Channel {
  return {
    id,
    kind: input.kind ?? 'webhook',
    name: input.name ?? '',
    enabled: input.enabled ?? true,
    trigger: 'capture',
    condition: null,
    config: input.config ?? {},
    template: null,
    per_hour: input.per_hour ?? 60,
    created_at: CREATED_AT,
  };
}

export function notificationHandlers(state: NotificationApiState): HttpHandler[] {
  return [
    http.get(CHANNELS, () => jsonResponse({ items: state.channels })),
    http.post(CHANNELS, async ({ request }) => {
      const input = (await readJsonBody<ChannelInput>(request)) ?? {};
      const created = channelFrom(input, `${NEW_ID_PREFIX}${String(state.channels.length + 10)}`);
      state.channels.push(created);
      return jsonResponse(created, 201);
    }),
    http.patch<ChannelPath>(CHANNEL, async ({ params, request }) => {
      const input = (await readJsonBody<ChannelInput>(request)) ?? {};
      const channelId = String(params.channelId);
      const index = state.channels.findIndex((item) => item.id === channelId);
      const current = state.channels[index];
      if (current === undefined) {
        return errorResponse(404, 'channel_not_found', 'Channel not found.');
      }
      state.patches.push(input);
      const updated: Channel = {
        ...current,
        name: input.name ?? current.name,
        enabled: input.enabled ?? current.enabled,
        per_hour: input.per_hour ?? current.per_hour,
        config: input.config === undefined ? current.config : { ...current.config, ...input.config },
      };
      state.channels[index] = updated;
      return jsonResponse(updated);
    }),
    http.delete<ChannelPath>(CHANNEL, ({ params }) => {
      const channelId = String(params.channelId);
      state.channels = state.channels.filter((item) => item.id !== channelId);
      return emptyResponse();
    }),
    http.post<ChannelPath>(CHANNEL_TEST, ({ params }) => {
      const channelId = String(params.channelId);
      if (state.testStatus === 'rate-limited') {
        return jsonResponse({ error: { code: 'quota_exceeded', message: 'Too many test messages.' } }, 429, {
          'Retry-After': '30',
        });
      }
      return jsonResponse(deliveryFor(channelId, state.testStatus), 202);
    }),
    http.get(SECRETS, () => jsonResponse({ items: state.secrets })),
    http.get(ADDRESSES, () => jsonResponse({ items: state.addresses })),
    http.post(ADDRESSES, async ({ request }) => {
      const input = (await readJsonBody<{ address: string }>(request)) ?? { address: '' };
      const created: EmailAddress = {
        id: `${NEW_ID_PREFIX}20`,
        address: input.address,
        verified: false,
        created_at: CREATED_AT,
      };
      state.addresses.push(created);
      return jsonResponse(created, 201);
    }),
    http.delete<AddressPath>(ADDRESS, ({ params }) => {
      const addressId = String(params.addressId);
      state.addresses = state.addresses.filter((item) => item.id !== addressId);
      return emptyResponse();
    }),
    http.post(VERIFY, async ({ request }) => {
      const input = (await readJsonBody<{ token: string }>(request)) ?? { token: '' };
      if (input.token !== VERIFY_TOKEN) {
        return errorResponse(422, 'invalid_token', 'The link is invalid or has expired.');
      }
      const address = state.addresses[0];
      if (address === undefined) {
        return errorResponse(422, 'invalid_token', 'The link is invalid or has expired.');
      }
      return jsonResponse({ ...address, verified: true });
    }),
    http.post(DISABLE, async ({ request }) => {
      const input = (await readJsonBody<{ token: string }>(request)) ?? { token: '' };
      if (input.token !== DISABLE_TOKEN) {
        return errorResponse(422, 'invalid_token', 'The link is invalid or has expired.');
      }
      return emptyResponse();
    }),
  ];
}
