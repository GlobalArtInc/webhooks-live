import { describe, expect, it } from 'vitest';
import type { Capture, CaptureSummary } from '../../api/types';
import { captureSeries } from '../endpoint/testFixtures';
import { summaryOf } from '../../test/msw/store';
import { refineCaptures } from './refine';

function summaryWith(seq: number, clientIp: string, path: string): CaptureSummary {
  const base = summaryOf(captureSeries(1)[0] as Capture);
  return { ...base, seq, client_ip: clientIp, path };
}

const ITEMS = [
  summaryWith(1, '203.0.113.7', '/orders/1'),
  summaryWith(2, '203.0.114.9', '/Orders/2'),
  summaryWith(3, '198.51.100.4', '/hooks'),
  summaryWith(4, '203.0.113.7', ''),
];

function seqsOf(items: CaptureSummary[]): number[] {
  return items.map((item) => item.seq);
}

describe('refineCaptures', () => {
  it('returns the same list when no sender or path is set', () => {
    expect(refineCaptures(ITEMS, { q: 'orders' })).toBe(ITEMS);
  });

  it('keeps exact senders and IPv4 ranges', () => {
    expect(seqsOf(refineCaptures(ITEMS, { ip: '203.0.113.7' }))).toEqual([1, 4]);
    expect(seqsOf(refineCaptures(ITEMS, { ip: '203.0.113.0/24' }))).toEqual([1, 4]);
    expect(seqsOf(refineCaptures(ITEMS, { ip: '203.0.0.0/8' }))).toEqual([1, 2, 4]);
  });

  it('matches path prefixes ignoring case and treats an empty path as the root', () => {
    expect(seqsOf(refineCaptures(ITEMS, { path: '/orders' }))).toEqual([1, 2]);
    expect(seqsOf(refineCaptures(ITEMS, { path: '/' }))).toEqual([1, 2, 3, 4]);
  });
});
