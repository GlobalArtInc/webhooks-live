import { describe, expect, it } from 'vitest';
import { dnsCaptureFixture, DNS_NAME } from '../../../test/fixtures/kinds';
import { dnsMetaOf, dnsTypeNumber, labelOf } from './dnsMeta';

describe('dnsMetaOf', () => {
  it('reads the documented fields and defaults the flags to off', () => {
    expect(dnsMetaOf(dnsCaptureFixture.meta)).toEqual({
      name: `x1.${DNS_NAME}`,
      type: 'A',
      class: 'IN',
      transport: 'udp',
      dnsId: 35614,
      flags: { rd: true, cd: false, do: true },
      ednsUdpSize: 4096,
      clientSubnet: null,
    });
    expect(dnsMetaOf({ name: 'x.example.test', type: 'TXT', class: 'IN' })?.flags).toEqual({
      rd: false,
      cd: false,
      do: false,
    });
  });

  it('refuses meta without a text name, type or class, or with an unknown transport', () => {
    expect(dnsMetaOf(null)).toBeNull();
    expect(dnsMetaOf({ name: 42, type: 'A', class: 'IN' })).toBeNull();
    expect(dnsMetaOf({ name: 'x.example.test', type: 'A' })).toBeNull();
    expect(dnsMetaOf({ name: 'x.example.test', type: 'A', class: 'IN', transport: 'quic' })?.transport).toBeNull();
    expect(dnsTypeNumber('AAAA')).toBe(28);
    expect(dnsTypeNumber('ZZZ')).toBeUndefined();
  });

  it('takes the label in front of the endpoint name, ignoring case, and nothing for another zone', () => {
    expect(labelOf('X1.Abc.dns.hooks.example.test', 'abc.DNS.hooks.example.test')).toBe('X1');
    expect(labelOf('x1.other.example.test', 'abc.dns.hooks.example.test')).toBeNull();
    expect(labelOf('abc.dns.hooks.example.test', 'abc.dns.hooks.example.test')).toBeNull();
  });
});
