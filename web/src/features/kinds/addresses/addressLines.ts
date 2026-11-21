import type { Endpoint } from '../../../api/types';

export type AddressSource = Pick<Endpoint, 'url' | 'email' | 'dns'>;

export function emailAddressOf(source: AddressSource): string | null {
  const email = source.email;
  return email?.enabled === true && email.address !== null && email.address !== '' ? email.address : null;
}

export function dnsNameOf(source: AddressSource): string | null {
  const dns = source.dns;
  return dns?.enabled === true && dns.name !== null && dns.name !== '' ? dns.name : null;
}

export function addressCountOf(source: AddressSource): number {
  return 1 + (emailAddressOf(source) === null ? 0 : 1) + (dnsNameOf(source) === null ? 0 : 1);
}
