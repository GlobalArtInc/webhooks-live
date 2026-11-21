import { describe, expect, it } from 'vitest';
import { cidrForAddress, isCovered, normalizeCidr, parseCidr } from './cidr';

describe('parseCidr', () => {
  it.each([
    ['203.0.113.0/24', true],
    ['203.0.113.7', true],
    ['0.0.0.0/0', true],
    ['2001:db8::/32', true],
    ['::1', true],
    ['::ffff:192.0.2.1', true],
    ['2001:0db8:0000:0000:0000:0000:0000:0001/128', true],
    ['256.0.0.1', false],
    ['010.0.0.1', false],
    ['1.2.3', false],
    ['203.0.113.0/', false],
    ['not an address', false],
    ['1::2::3', false],
    ['12345::1', false],
    ['fe80::1%eth0', false],
    ['1.2.3.4/24/8', false],
    ['', false],
  ])('accepts or rejects %s', (input, valid) => {
    expect(parseCidr(input).ok).toBe(valid);
  });

  it('rejects a prefix longer than the address family allows', () => {
    expect(parseCidr('203.0.113.0/33')).toMatchObject({ ok: false, problem: 'prefix' });
    expect(parseCidr('2001:db8::/129')).toMatchObject({ ok: false, problem: 'prefix' });
    expect(parseCidr('203.0.113.0/x')).toMatchObject({ ok: false, problem: 'prefix' });
  });

  it('rejects host bits after the prefix and suggests the network', () => {
    expect(parseCidr('203.0.113.5/24')).toMatchObject({
      ok: false,
      problem: 'hostBits',
      suggestion: '203.0.113.0/24',
    });
    expect(parseCidr('2001:db8::1/32')).toMatchObject({ ok: false, problem: 'hostBits', suggestion: '2001:db8::/32' });
  });
});

describe('normalizeCidr', () => {
  it('adds the full prefix to a bare address and compresses IPv6', () => {
    expect(normalizeCidr(' 198.51.100.4 ')).toBe('198.51.100.4/32');
    expect(normalizeCidr('2001:0DB8:0:0:0:0:0:1')).toBe('2001:db8::1/128');
  });

  it('keeps an invalid entry as typed so the validator can report it', () => {
    expect(normalizeCidr(' nope ')).toBe('nope');
  });
});

describe('cidrForAddress and isCovered', () => {
  it('turns an address into a single-host range and refuses ranges', () => {
    expect(cidrForAddress('198.51.100.4')).toBe('198.51.100.4/32');
    expect(cidrForAddress('198.51.100.0/24')).toBeNull();
    expect(cidrForAddress('nope')).toBeNull();
  });

  it('finds an address inside a range and ignores the other family', () => {
    expect(isCovered(['198.51.100.0/24'], '198.51.100.4')).toBe(true);
    expect(isCovered(['198.51.101.0/24'], '198.51.100.4')).toBe(false);
    expect(isCovered(['2001:db8::/32'], '198.51.100.4')).toBe(false);
    expect(isCovered(['2001:db8::/32'], '2001:db8:1::5')).toBe(true);
    expect(isCovered([], '198.51.100.4')).toBe(false);
  });

  it('reads an IPv4-mapped address as the IPv4 address it carries', () => {
    expect(cidrForAddress('::ffff:198.51.100.4')).toBe('198.51.100.4/32');
    expect(isCovered(['198.51.100.0/24'], '::ffff:198.51.100.4')).toBe(true);
    expect(isCovered(['198.51.101.0/24'], '::ffff:198.51.100.4')).toBe(false);
    expect(isCovered(['2001:db8::/32'], '::ffff:198.51.100.4')).toBe(false);
  });
});
