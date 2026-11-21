import { afterEach, describe, expect, it } from 'vitest';
import { clearMarkedCapture, getMarkedCapture } from '../diff';
import { activateCompare, compareLabelOf } from './compare';

const ENDPOINT = 'compare-test-endpoint';
const FIRST = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a01';
const SECOND = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a02';

describe('compare actions', () => {
  afterEach(() => {
    clearMarkedCapture(ENDPOINT);
  });

  it('labels the row by the current mark', () => {
    expect(compareLabelOf(null, FIRST)).toBe('mark');
    expect(compareLabelOf(FIRST, FIRST)).toBe('unmark');
    expect(compareLabelOf(FIRST, SECOND)).toBe('withMarked');
  });

  it('marks the first capture, unmarks it when chosen again, and returns the diff route for a second one', () => {
    expect(activateCompare(ENDPOINT, FIRST, null)).toBeNull();
    expect(getMarkedCapture(ENDPOINT)).toBe(FIRST);

    expect(activateCompare(ENDPOINT, FIRST, FIRST)).toBeNull();
    expect(getMarkedCapture(ENDPOINT)).toBeNull();

    activateCompare(ENDPOINT, FIRST, null);
    expect(activateCompare(ENDPOINT, SECOND, FIRST)).toEqual({ id: ENDPOINT, a: FIRST, b: SECOND });
  });
});
