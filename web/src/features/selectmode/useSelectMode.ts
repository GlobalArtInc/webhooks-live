import { useCallback, useState } from 'react';
import type { CaptureSummary } from '../../api/types';
import {
  beginSelection,
  checkCapture,
  checkedIn,
  EMPTY_SELECTION,
  endSelection,
  scopedTo,
  selectAll,
  toggleCapture,
  type SelectState,
} from './selection';

export interface SelectMode {
  active: boolean;
  ids: ReadonlySet<string>;
  begin: () => void;
  end: () => void;
  toggle: (captureId: string) => void;
  check: (captureId: string) => void;
  selectAll: (captureIds: readonly string[]) => void;
  checkedIn: (items: readonly CaptureSummary[]) => CaptureSummary[];
}

export function useSelectMode(endpointId: string): SelectMode {
  const [stored, setStored] = useState<SelectState>(EMPTY_SELECTION);
  const state = scopedTo(stored, endpointId);

  const update = useCallback(
    (change: (current: SelectState) => SelectState) => {
      setStored((current) => change(scopedTo(current, endpointId)));
    },
    [endpointId],
  );

  return {
    active: state.active,
    ids: state.ids,
    begin: useCallback(() => {
      update(beginSelection);
    }, [update]),
    end: useCallback(() => {
      update(endSelection);
    }, [update]),
    toggle: useCallback(
      (captureId: string) => {
        update((current) => toggleCapture(current, captureId));
      },
      [update],
    ),
    check: useCallback(
      (captureId: string) => {
        update((current) => checkCapture(current, captureId));
      },
      [update],
    ),
    selectAll: useCallback(
      (captureIds: readonly string[]) => {
        update((current) => selectAll(current, captureIds));
      },
      [update],
    ),
    checkedIn: (items: readonly CaptureSummary[]) => checkedIn(state, items),
  };
}
