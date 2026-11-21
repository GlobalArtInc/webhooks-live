import type { Capture, Endpoint } from '../../api/types';
import { ENDPOINT_ID, endpointFixture } from './endpoint';

const MAIL_DOMAIN = 'in.hooks.example.test';
const DNS_ZONE = 'dns.hooks.example.test';

export const MAIL_ADDRESS = `${ENDPOINT_ID}@${MAIL_DOMAIN}`;
export const DNS_NAME = `${ENDPOINT_ID}.${DNS_ZONE}`;

const KIND_BASE: Capture = {
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5d00',
  seq: 1,
  endpoint_id: ENDPOINT_ID,
  received_at: '2026-11-21T10:17:00.000Z',
  method: 'POST',
  path: '',
  query_string: '',
  client_ip: '198.51.100.25',
  content_type: null,
  body_size: 38,
  status_override: null,
  response_status: 202,
  forward_state: null,
  url: '',
  host: '',
  headers: [],
  query: [],
  form: null,
  body: { encoding: 'utf8', data: 'Hello from the test fixture.', truncated: false },
  forwards: [],
};

export const emailCaptureFixture: Capture = {
  ...KIND_BASE,
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5d01',
  seq: 2,
  method: 'SMTP',
  client_ip: '192.0.2.10',
  kind: 'email',
  subject: 'Invoice 2026-11',
  meta: {
    envelope_from: 'billing@shop.example.test',
    envelope_to: [MAIL_ADDRESS],
    helo: 'mx.shop.example.test',
    tls: true,
    tag: null,
    subject: 'Invoice 2026-11',
    from: ['billing@shop.example.test'],
    to: [MAIL_ADDRESS],
    cc: [],
    date: '2026-11-21T10:16:58.000Z',
    message_id: null,
    parts: [],
  },
};

export const emailWithoutSubjectFixture: Capture = {
  ...emailCaptureFixture,
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5d02',
  seq: 3,
  subject: null,
};

export const dnsCaptureFixture: Capture = {
  ...KIND_BASE,
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5d03',
  seq: 4,
  method: 'DNS',
  path: '',
  client_ip: '198.51.100.53',
  kind: 'dns',
  subject: `x1.${DNS_NAME}`,
  meta: {
    name: `x1.${DNS_NAME}`,
    type: 'A',
    class: 'IN',
    transport: 'udp',
    dns_id: 35614,
    flags: { rd: true, cd: false, do: true },
    edns_udp_size: 4096,
    client_subnet: null,
  },
};

export const scheduleCaptureFixture: Capture = {
  ...KIND_BASE,
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5d04',
  seq: 5,
  method: 'POST',
  path: '/stripe',
  kind: 'schedule',
  subject: 'Nightly sync',
  response_status: 200,
  meta: {
    schedule_id: '0193a1c2-1111-7a3e-8b44-1c2d3e4f5a01',
    schedule_name: 'Nightly sync',
    request: { method: 'POST', url: 'https://crm.example.test/hooks/sync' },
    duration_ms: 84,
    final_url: 'https://crm.example.test/hooks/sync',
    error: null,
    truncated: false,
  },
};

export const mailEndpointFixture: Endpoint = {
  ...endpointFixture,
  email: { enabled: true, address: MAIL_ADDRESS },
  dns: { enabled: true, name: DNS_NAME },
};
