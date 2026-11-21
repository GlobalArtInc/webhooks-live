import { clearMarkedCapture, compareWith, markCapture, type DiffRouteParams } from '../diff';

export type CompareLabel = 'mark' | 'unmark' | 'withMarked';

export function compareLabelOf(markedId: string | null, captureId: string): CompareLabel {
  if (markedId === null) {
    return 'mark';
  }
  return markedId === captureId ? 'unmark' : 'withMarked';
}

export function activateCompare(
  endpointId: string,
  captureId: string,
  markedId: string | null,
): DiffRouteParams | null {
  if (markedId === null) {
    markCapture(endpointId, captureId);
    return null;
  }
  if (markedId === captureId) {
    clearMarkedCapture(endpointId);
    return null;
  }
  return compareWith(endpointId, captureId);
}
