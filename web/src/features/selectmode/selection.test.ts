import { describe, expect, it } from 'vitest';
import { captureSeries } from '../endpoint/testFixtures';
import { summaryOf } from '../../test/msw/store';
import {
  beginSelection,
  checkedIn,
  EMPTY_SELECTION,
  endSelection,
  scopedTo,
  selectAll,
  toggleCapture,
} from './selection';

const items = captureSeries(4).map(summaryOf);
const [first, second, third] = items.map((item) => item.id);

describe('select mode model', () => {
  it('starts select mode with the first toggle and clears it when toggled off', () => {
    const checked = toggleCapture(EMPTY_SELECTION, first ?? '');
    expect(checked.active).toBe(true);
    expect(checkedIn(checked, items).map((item) => item.id)).toEqual([first]);

    const unchecked = toggleCapture(checked, first ?? '');
    expect(unchecked.active).toBe(true);
    expect(checkedIn(unchecked, items)).toEqual([]);
  });

  it('adds every visible capture to the checks and keeps the list order', () => {
    const begun = beginSelection(toggleCapture(EMPTY_SELECTION, third ?? ''));
    const all = selectAll(
      begun,
      items.map((item) => item.id),
    );
    expect(checkedIn(all, items).map((item) => item.id)).toEqual(items.map((item) => item.id));
    expect(checkedIn(endSelection(all), items)).toEqual([]);
  });

  it('forgets the checks of another endpoint', () => {
    const checked = toggleCapture(EMPTY_SELECTION, second ?? '');
    const elsewhere = scopedTo({ ...checked, endpointId: 'first-endpoint' }, 'second-endpoint');
    expect(elsewhere.active).toBe(false);
    expect(elsewhere.ids.size).toBe(0);
  });
});
