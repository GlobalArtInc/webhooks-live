import type { CaptureSummary } from '../../api/types';
import type { CaptureSearch } from '../filters/captureSearch';

export function hasRefinement(search: CaptureSearch): boolean {
  return search.ip !== undefined || search.path !== undefined;
}

export interface ClientFilter {
  ip?: string;
  path?: string;
}

export function matchesClientFilter(summary: CaptureSummary, filter: ClientFilter): boolean {
  return matchesIp(summary.client_ip, filter.ip) && matchesPath(summary.path, filter.path);
}

export function refineCaptures(items: CaptureSummary[], search: CaptureSearch): CaptureSummary[] {
  if (!hasRefinement(search)) {
    return items;
  }
  return items.filter((item) => matchesClientFilter(item, search));
}

function matchesIp(clientIp: string, filter: string | undefined): boolean {
  if (filter === undefined) {
    return true;
  }
  if (!filter.includes('/')) {
    return clientIp.toLowerCase() === filter;
  }
  return inIpv4Range(clientIp, filter);
}

function matchesPath(path: string, prefix: string | undefined): boolean {
  if (prefix === undefined) {
    return true;
  }
  const actual = path === '' ? '/' : path;
  return actual.toLowerCase().startsWith(prefix.toLowerCase());
}

function ipv4Of(address: string): number | undefined {
  const octets = address.split('.');
  if (octets.length !== 4) {
    return undefined;
  }
  let value = 0;
  for (const octet of octets) {
    if (!/^\d{1,3}$/.test(octet) || Number(octet) > 255) {
      return undefined;
    }
    value = value * 256 + Number(octet);
  }
  return value;
}

function inIpv4Range(address: string, range: string): boolean {
  const [network = '', bitsText = '32'] = range.split('/');
  const bits = Number(bitsText);
  const value = ipv4Of(address);
  const base = ipv4Of(network);
  if (value === undefined || base === undefined || bits > 32) {
    return false;
  }
  const mask = bits === 0 ? 0 : (0xffffffff << (32 - bits)) >>> 0;
  return (value & mask) >>> 0 === (base & mask) >>> 0;
}
