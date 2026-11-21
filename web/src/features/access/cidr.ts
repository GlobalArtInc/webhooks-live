export type IpFamily = 4 | 6;
export type CidrProblem = 'format' | 'prefix' | 'hostBits';

export interface ParsedCidr {
  family: IpFamily;
  address: bigint;
  prefix: number;
}

export interface CidrOk {
  ok: true;
  cidr: ParsedCidr;
}

export interface CidrFailure {
  ok: false;
  problem: CidrProblem;
  suggestion: string | null;
}

export type CidrResult = CidrOk | CidrFailure;

interface ZeroRun {
  start: number;
  length: number;
}

const BITS: Record<IpFamily, number> = { 4: 32, 6: 128 };
const DECIMAL = /^\d{1,3}$/;
const DIGITS = /^\d+$/;
const HEX_GROUP = /^[0-9a-fA-F]{1,4}$/;
const IPV6_CHARS = /^[0-9a-fA-F:.]+$/;
const IPV4_OCTETS = 4;
const IPV6_GROUPS = 8;
const OCTET_MAX = 255;
const PREFIX_MAX_DIGITS = 3;
const GROUP_BITS = 16n;
const GROUP_MASK = 0xffffn;
const OCTET_BITS = 8n;
const OCTET_MASK = 0xffn;
const IPV4_MAPPED_PREFIX = 0xffffn;
const IPV4_BITS = 32n;
const NO_RUN: ZeroRun = { start: -1, length: 0 };

function failure(problem: CidrProblem, suggestion: string | null = null): CidrFailure {
  return { ok: false, problem, suggestion };
}

export function parseIpv4(text: string): bigint | null {
  const parts = text.split('.');
  if (parts.length !== IPV4_OCTETS) {
    return null;
  }
  let value = 0n;
  for (const part of parts) {
    if (!DECIMAL.test(part) || (part.length > 1 && part.startsWith('0')) || Number(part) > OCTET_MAX) {
      return null;
    }
    value = (value << OCTET_BITS) | BigInt(Number(part));
  }
  return value;
}

function expandIpv4Tail(text: string): string | null {
  const split = text.lastIndexOf(':');
  const tail = text.slice(split + 1);
  if (!tail.includes('.')) {
    return text;
  }
  const value = parseIpv4(tail);
  if (value === null) {
    return null;
  }
  const high = (value >> GROUP_BITS).toString(16);
  const low = (value & GROUP_MASK).toString(16);
  return `${text.slice(0, split + 1)}${high}:${low}`;
}

function parseGroups(text: string): bigint[] | null {
  if (text === '') {
    return [];
  }
  const groups: bigint[] = [];
  for (const part of text.split(':')) {
    if (!HEX_GROUP.test(part)) {
      return null;
    }
    groups.push(BigInt(`0x${part}`));
  }
  return groups;
}

export function parseIpv6(text: string): bigint | null {
  if (!IPV6_CHARS.test(text)) {
    return null;
  }
  const expanded = expandIpv4Tail(text);
  if (expanded === null) {
    return null;
  }
  const sides = expanded.split('::');
  if (sides.length > 2) {
    return null;
  }
  const head = parseGroups(sides[0] ?? '');
  const tail = sides.length === 2 ? parseGroups(sides[1] ?? '') : [];
  if (head === null || tail === null) {
    return null;
  }
  const filled = head.length + tail.length;
  const compressed = sides.length === 2;
  if (compressed ? filled >= IPV6_GROUPS : filled !== IPV6_GROUPS) {
    return null;
  }
  const zeros = Array.from({ length: IPV6_GROUPS - filled }, () => 0n);
  return [...head, ...zeros, ...tail].reduce((value, group) => (value << GROUP_BITS) | group, 0n);
}

function hostMask(family: IpFamily, prefix: number): bigint {
  return (1n << BigInt(BITS[family] - prefix)) - 1n;
}

function formatIpv4(address: bigint): string {
  return [24n, 16n, 8n, 0n].map((shift) => String((address >> shift) & OCTET_MASK)).join('.');
}

function longestZeroRun(groups: readonly bigint[]): ZeroRun {
  let best: ZeroRun = NO_RUN;
  let start = -1;
  groups.forEach((group, index) => {
    if (group !== 0n) {
      start = -1;
      return;
    }
    if (start === -1) {
      start = index;
    }
    if (index - start + 1 > best.length) {
      best = { start, length: index - start + 1 };
    }
  });
  return best.length >= 2 ? best : NO_RUN;
}

function groupsOf(address: bigint): bigint[] {
  return Array.from(
    { length: IPV6_GROUPS },
    (_, index) => (address >> (BigInt(IPV6_GROUPS - 1 - index) * GROUP_BITS)) & GROUP_MASK,
  );
}

function joinGroups(groups: readonly bigint[]): string {
  return groups.map((group) => group.toString(16)).join(':');
}

function formatIpv6(address: bigint): string {
  const groups = groupsOf(address);
  const run = longestZeroRun(groups);
  if (run.start === -1) {
    return joinGroups(groups);
  }
  return `${joinGroups(groups.slice(0, run.start))}::${joinGroups(groups.slice(run.start + run.length))}`;
}

export function formatCidr(cidr: ParsedCidr): string {
  const address = cidr.family === 4 ? formatIpv4(cidr.address) : formatIpv6(cidr.address);
  return `${address}/${cidr.prefix}`;
}

export function parseCidr(input: string): CidrResult {
  const text = input.trim();
  const parts = text.split('/');
  if (parts.length > 2 || text === '') {
    return failure('format');
  }
  const host = parts[0] ?? '';
  const family: IpFamily = host.includes(':') ? 6 : 4;
  const address = family === 4 ? parseIpv4(host) : parseIpv6(host);
  if (address === null) {
    return failure('format');
  }
  const bits = BITS[family];
  const prefixText = parts[1];
  if (prefixText === undefined) {
    return { ok: true, cidr: { family, address, prefix: bits } };
  }
  if (!DIGITS.test(prefixText) || prefixText.length > PREFIX_MAX_DIGITS || Number(prefixText) > bits) {
    return failure('prefix');
  }
  const prefix = Number(prefixText);
  const mask = hostMask(family, prefix);
  if ((address & mask) !== 0n) {
    return failure('hostBits', formatCidr({ family, address: address & ~mask, prefix }));
  }
  return { ok: true, cidr: { family, address, prefix } };
}

export function normalizeCidr(input: string): string {
  const parsed = parseCidr(input);
  return parsed.ok ? formatCidr(parsed.cidr) : input.trim();
}

function unmapIpv4(cidr: ParsedCidr): ParsedCidr {
  if (cidr.family === 6 && cidr.prefix === BITS[6] && cidr.address >> IPV4_BITS === IPV4_MAPPED_PREFIX) {
    return { family: 4, address: cidr.address & ((1n << IPV4_BITS) - 1n), prefix: BITS[4] };
  }
  return cidr;
}

export function cidrForAddress(address: string): string | null {
  const parsed = parseCidr(address);
  return parsed.ok && !address.includes('/') ? formatCidr(unmapIpv4(parsed.cidr)) : null;
}

export function isCovered(entries: readonly string[], address: string): boolean {
  const target = parseCidr(address);
  if (!target.ok) {
    return false;
  }
  const host = unmapIpv4(target.cidr);
  return entries.some((entry) => {
    const parsed = parseCidr(entry);
    if (!parsed.ok || parsed.cidr.family !== host.family) {
      return false;
    }
    const mask = hostMask(parsed.cidr.family, parsed.cidr.prefix);
    return (parsed.cidr.address | mask) === (host.address | mask);
  });
}
