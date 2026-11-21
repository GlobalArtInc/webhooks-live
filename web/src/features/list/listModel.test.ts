import { describe, expect, it } from 'vitest';
import type { CaptureListData } from '../../api/queries';
import type { CapturePage } from '../../api/types';
import { captureIdFor, summaryAt } from '../endpoint/testFixtures';
import { flattenCaptures, neighbourOf, restoreCaptureInPages, totalOf } from './listModel';

function pageOf(seqs: number[], total: number, nextCursor: string | null = null): CapturePage {
  return { items: seqs.map(summaryAt), next_cursor: nextCursor, total };
}

function dataOf(pages: CapturePage[]): CaptureListData {
  return { pages, pageParams: pages.map((_page, index) => (index === 0 ? null : `cursor-${index}`)) };
}

function seqsOf(data: CaptureListData): number[] {
  return flattenCaptures(data).map((item) => item.seq);
}

describe('list model', () => {
  it('flattens the loaded pages in order and reads the total of the first page', () => {
    const data = dataOf([pageOf([5, 4], 5, 'next'), pageOf([3, 2, 1], 5)]);

    expect(seqsOf(data)).toEqual([5, 4, 3, 2, 1]);
    expect(totalOf(data)).toBe(5);
    expect(flattenCaptures(undefined)).toEqual([]);
    expect(totalOf(undefined)).toBeNull();
  });

  it('picks the older neighbour, falls back to the newer one at the end, and has none for a lone capture', () => {
    const items = [3, 2, 1].map(summaryAt);

    expect(neighbourOf(items, captureIdFor(2))).toBe(captureIdFor(1));
    expect(neighbourOf(items, captureIdFor(1))).toBe(captureIdFor(2));
    expect(neighbourOf([summaryAt(1)], captureIdFor(1))).toBeNull();
    expect(neighbourOf(items, captureIdFor(9))).toBeNull();
  });

  it('restores a removed capture at its sorted position and bumps the total of every page', () => {
    const data = dataOf([pageOf([5, 4, 2], 4, 'next'), pageOf([1], 4)]);

    const restored = restoreCaptureInPages(data, summaryAt(3));

    expect(seqsOf(restored)).toEqual([5, 4, 3, 2, 1]);
    expect(totalOf(restored)).toBe(5);
  });

  it('appends the oldest capture after the last loaded item and leaves listed captures alone', () => {
    const data = dataOf([pageOf([3, 2], 2)]);

    expect(seqsOf(restoreCaptureInPages(data, summaryAt(1)))).toEqual([3, 2, 1]);
    expect(restoreCaptureInPages(data, summaryAt(3))).toBe(data);
  });
});
