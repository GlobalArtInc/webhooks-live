import { describe, expect, it } from 'vitest';
import { mailEndpointFixture } from '../../test/fixtures/kinds';
import { endpointFixture } from '../../test/fixtures/endpoint';
import { addressCountOf, dnsNameOf, emailAddressOf } from './addresses/addressLines';
import { badgeKindOf, captureKindOf } from './kinds';
import { digSampleOf, emailSampleOf } from './snippets';

describe('captureKindOf and badgeKindOf', () => {
  it('treats a summary without a kind as HTTP and keeps the known kinds', () => {
    expect(captureKindOf({})).toBe('http');
    expect(captureKindOf({ kind: 'dns' })).toBe('dns');
    expect(captureKindOf({ kind: 'schedule' })).toBe('schedule');
  });

  it('gives a badge only to email and DNS captures', () => {
    expect(badgeKindOf('email')).toBe('email');
    expect(badgeKindOf('dns')).toBe('dns');
    expect(badgeKindOf('http')).toBeNull();
    expect(badgeKindOf('schedule')).toBeNull();
  });
});

describe('address lines', () => {
  it('lists only the enabled addresses of the endpoint', () => {
    expect(addressCountOf(mailEndpointFixture)).toBe(3);
    expect(addressCountOf(endpointFixture)).toBe(1);
    expect(emailAddressOf({ ...mailEndpointFixture, email: { enabled: false, address: 'x@example.test' } })).toBeNull();
    expect(dnsNameOf({ ...mailEndpointFixture, dns: { enabled: true, name: null } })).toBeNull();
  });

  it('builds the sample commands with the address quoted for the shell', () => {
    expect(emailSampleOf("o'brien@in.hooks.example.test")).toBe(
      `swaks --to 'o'\\''brien@in.hooks.example.test' --header 'Subject: Test message' --body 'Hello from webhooks.live'`,
    );
    expect(digSampleOf('abc.dns.hooks.example.test')).toBe("dig 'test.abc.dns.hooks.example.test' A");
  });
});
