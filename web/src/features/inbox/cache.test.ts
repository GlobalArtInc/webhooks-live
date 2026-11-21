import { describe, expect, it } from 'vitest';
import { summaryFor } from '../live/liveFixtures';
import { mergeInboxCapture, removeInboxCapture, type InboxListData } from './cache';

const OTHER_ENDPOINT = '0123456789abcdefghjkmnpr';

function listOf(seqs: number[]): InboxListData {
  return {
    pages: [{ items: seqs.map((seq) => summaryFor(seq)), next_cursor: 'next' }],
    pageParams: [null],
  };
}

describe('inbox list cache', () => {
  it('puts a new capture at the top of the first page without duplicating a known one', () => {
    const data = listOf([2, 1]);
    const arrived = summaryFor(3, { endpoint_id: OTHER_ENDPOINT });
    const merged = mergeInboxCapture(data, arrived);
    expect(merged.pages[0]?.items.map((item) => item.id)).toEqual([arrived.id, summaryFor(2).id, summaryFor(1).id]);
    const again = mergeInboxCapture(merged, arrived);
    expect(again.pages[0]?.items).toHaveLength(3);
    expect(again.pages[0]?.next_cursor).toBe('next');
  });

  it('removes a capture by id and leaves the list alone when it is unknown', () => {
    const data = listOf([2, 1]);
    const removed = removeInboxCapture(data, summaryFor(2).id);
    expect(removed.pages[0]?.items.map((item) => item.seq)).toEqual([1]);
    expect(removeInboxCapture(data, summaryFor(9).id)).toBe(data);
  });
});
