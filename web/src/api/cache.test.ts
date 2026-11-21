import { describe, expect, it } from 'vitest';
import {
  binaryCaptureFixture,
  formCaptureFixture,
  forwardedCaptureFixture,
  jsonCaptureFixture,
} from '../test/fixtures/captures';
import { endpointFixture } from '../test/fixtures/endpoint';
import { type CaptureListData, addCaptureToEndpoint, mergeCaptureIntoPages, removeCaptureFromPages } from './queries';
import type { CaptureSummary } from './types';

const NEWER_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a09';

function twoPageList(total: number): CaptureListData {
  return {
    pages: [
      { items: [forwardedCaptureFixture, binaryCaptureFixture], next_cursor: 'cursor-binary', total },
      { items: [formCaptureFixture, jsonCaptureFixture], next_cursor: null, total },
    ],
    pageParams: [null, 'cursor-binary'],
  };
}

function captureIds(data: CaptureListData): string[] {
  return data.pages.flatMap((page) => page.items.map((item) => item.id));
}

const NEWEST_FIRST_IDS = [
  forwardedCaptureFixture.id,
  binaryCaptureFixture.id,
  formCaptureFixture.id,
  jsonCaptureFixture.id,
];

describe('capture cache helpers', () => {
  it('merge puts a new capture first in a newest-first list and keeps the other pages in order', () => {
    const newer: CaptureSummary = { ...jsonCaptureFixture, id: NEWER_ID, seq: 5 };

    const merged = mergeCaptureIntoPages(twoPageList(4), newer, 'desc');

    expect(captureIds(merged)).toEqual([NEWER_ID, ...NEWEST_FIRST_IDS]);
    expect(merged.pages.map((page) => page.items.length)).toEqual([3, 2]);
    expect(merged.pages.map((page) => page.total)).toEqual([5, 5]);
  });

  it('merge replaces a capture that is already cached without moving it or changing the total', () => {
    const changed: CaptureSummary = { ...binaryCaptureFixture, method: 'PATCH' };

    const merged = mergeCaptureIntoPages(twoPageList(4), changed, 'desc');

    expect(captureIds(merged)).toEqual(NEWEST_FIRST_IDS);
    expect(merged.pages[0]?.items[1]?.method).toBe('PATCH');
    expect(merged.pages[0]?.total).toBe(4);
  });

  it('remove drops a capture by id from the page that holds it', () => {
    const removed = removeCaptureFromPages(twoPageList(4), binaryCaptureFixture.id);

    expect(captureIds(removed)).toEqual([forwardedCaptureFixture.id, formCaptureFixture.id, jsonCaptureFixture.id]);
    expect(removed.pages.map((page) => page.total)).toEqual([3, 3]);
  });

  it('remove returns the same data when the capture is not cached', () => {
    const data = twoPageList(4);

    expect(removeCaptureFromPages(data, NEWER_ID)).toBe(data);
  });

  it('counts a capture once when the same event arrives twice', () => {
    const once = addCaptureToEndpoint(endpointFixture, jsonCaptureFixture);
    const twice = addCaptureToEndpoint(once, jsonCaptureFixture);

    expect(twice.capture_count).toBe(1);
    expect(twice.last_seq).toBe(jsonCaptureFixture.seq);
  });
});
