import type { CaptureSummary, Endpoint } from '../../api/types';
import { endpointFixture } from './endpoint';
import { PUBLIC_URL } from './instance';

export const PAYMENTS_ENDPOINT_ID = '0123456789abcdefghjkmnpq';
export const SHIPPING_ENDPOINT_ID = '0123456789abcdefghjkmnpr';
export const PAYMENTS_TITLE = 'Payments hook';
export const SHIPPING_TITLE = 'Shipping hook';

const CAPTURE_ID_PREFIX = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a';
const RECEIVED_AT = '2026-11-21T10:00:00.000Z';

export const paymentsEndpoint: Endpoint = {
  ...endpointFixture,
  id: PAYMENTS_ENDPOINT_ID,
  url: `${PUBLIC_URL}/${PAYMENTS_ENDPOINT_ID}`,
  title: PAYMENTS_TITLE,
  my_role: 'owner',
};

export const shippingEndpoint: Endpoint = {
  ...endpointFixture,
  id: SHIPPING_ENDPOINT_ID,
  url: `${PUBLIC_URL}/${SHIPPING_ENDPOINT_ID}`,
  title: SHIPPING_TITLE,
  my_role: 'viewer',
};

export function captureIdOf(seq: number): string {
  return `${CAPTURE_ID_PREFIX}${String(seq).padStart(2, '0')}`;
}

export function inboxCapture(endpointId: string, seq: number, path: string): CaptureSummary {
  return {
    id: captureIdOf(seq),
    seq,
    endpoint_id: endpointId,
    received_at: new Date(Date.parse(RECEIVED_AT) + seq * 1000).toISOString(),
    method: 'POST',
    path,
    query_string: '',
    client_ip: '203.0.113.7',
    content_type: 'application/json',
    body_size: 42,
    status_override: null,
    response_status: 200,
    forward_state: null,
  };
}

export const inboxCaptures: CaptureSummary[] = [
  inboxCapture(PAYMENTS_ENDPOINT_ID, 1, '/hooks/payments'),
  inboxCapture(SHIPPING_ENDPOINT_ID, 2, '/hooks/shipping'),
  inboxCapture(PAYMENTS_ENDPOINT_ID, 3, '/hooks/payments/refund'),
];
