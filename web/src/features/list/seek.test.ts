import { describe, expect, it } from 'vitest';
import { captureIdFor, summaryAt } from '../endpoint/testFixtures';
import { resolveSeek } from './seek';

const items = [3, 2, 1].map(summaryAt);

describe('resolveSeek', () => {
  it('finishes a load target once the capture is listed and waits while more pages may hold it', () => {
    expect(resolveSeek(items, true, { kind: 'load', captureId: captureIdFor(2) })).toEqual({
      status: 'done',
      captureId: null,
    });
    expect(resolveSeek(items, true, { kind: 'load', captureId: captureIdFor(9) })).toEqual({ status: 'pending' });
    expect(resolveSeek(items, false, { kind: 'load', captureId: captureIdFor(9) })).toEqual({
      status: 'done',
      captureId: null,
    });
  });

  it('selects the older neighbour, or waits for the next page when the loaded end is reached', () => {
    expect(resolveSeek(items, false, { kind: 'next', captureId: captureIdFor(3) })).toEqual({
      status: 'done',
      captureId: captureIdFor(2),
    });
    expect(resolveSeek(items, true, { kind: 'next', captureId: captureIdFor(1) })).toEqual({ status: 'pending' });
    expect(resolveSeek(items, false, { kind: 'next', captureId: captureIdFor(1) })).toEqual({
      status: 'done',
      captureId: null,
    });
  });

  it('selects the oldest listed capture only once no further page exists', () => {
    expect(resolveSeek(items, true, { kind: 'last' })).toEqual({ status: 'pending' });
    expect(resolveSeek(items, false, { kind: 'last' })).toEqual({ status: 'done', captureId: captureIdFor(1) });
    expect(resolveSeek([], false, { kind: 'last' })).toEqual({ status: 'done', captureId: null });
  });
});
