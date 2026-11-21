import type { CaptureMeta } from '../../../api/types';

export type DnsTransport = 'udp' | 'tcp';

export interface DnsFlags {
  rd: boolean;
  cd: boolean;
  do: boolean;
}

export interface DnsMeta {
  name: string;
  type: string;
  class: string;
  transport: DnsTransport | null;
  dnsId: number | null;
  flags: DnsFlags;
  ednsUdpSize: number | null;
  clientSubnet: string | null;
}

const TYPE_NUMBERS: Partial<Readonly<Record<string, number>>> = {
  A: 1,
  NS: 2,
  CNAME: 5,
  SOA: 6,
  PTR: 12,
  MX: 15,
  TXT: 16,
  AAAA: 28,
  SRV: 33,
  CAA: 257,
};

const EMPTY_META: CaptureMeta = {};

function isRecord(value: unknown): value is CaptureMeta {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function stringOf(meta: CaptureMeta, key: string): string | null {
  const value = meta[key];
  return typeof value === 'string' ? value : null;
}

function numberOf(meta: CaptureMeta, key: string): number | null {
  const value = meta[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function flagsOf(value: unknown): DnsFlags {
  const flags: CaptureMeta = isRecord(value) ? value : EMPTY_META;
  return { rd: flags['rd'] === true, cd: flags['cd'] === true, do: flags['do'] === true };
}

function transportOf(value: unknown): DnsTransport | null {
  return value === 'udp' || value === 'tcp' ? value : null;
}

export function dnsMetaOf(meta: CaptureMeta | null | undefined): DnsMeta | null {
  if (meta === null || meta === undefined) {
    return null;
  }
  const name = stringOf(meta, 'name');
  const type = stringOf(meta, 'type');
  const dnsClass = stringOf(meta, 'class');
  if (name === null || type === null || dnsClass === null) {
    return null;
  }
  return {
    name,
    type,
    class: dnsClass,
    transport: transportOf(meta['transport']),
    dnsId: numberOf(meta, 'dns_id'),
    flags: flagsOf(meta['flags']),
    ednsUdpSize: numberOf(meta, 'edns_udp_size'),
    clientSubnet: stringOf(meta, 'client_subnet'),
  };
}

export function dnsTypeNumber(type: string): number | undefined {
  return TYPE_NUMBERS[type];
}

export function labelOf(name: string, zone: string | null | undefined): string | null {
  if (zone === null || zone === undefined || zone === '') {
    return null;
  }
  const suffix = `.${zone.toLowerCase()}`;
  const lowered = name.toLowerCase();
  return lowered.endsWith(suffix) && lowered.length > suffix.length ? name.slice(0, -suffix.length) : null;
}
