import { configure } from '@testing-library/react';

const ASYNC_UTIL_TIMEOUT_MS = 15_000;

configure({ asyncUtilTimeout: ASYNC_UTIL_TIMEOUT_MS });

function emptyRects(): DOMRectList {
  const rects: DOMRect[] = [];
  return Object.assign(rects, { item: () => null }) as unknown as DOMRectList;
}

function zeroRect(): DOMRect {
  return new DOMRect(0, 0, 0, 0);
}

if (typeof document !== 'undefined') {
  if (typeof document.execCommand !== 'function') {
    Object.defineProperty(document, 'execCommand', { value: () => false, configurable: true });
  }
  if (typeof Range.prototype.getClientRects !== 'function') {
    Range.prototype.getClientRects = emptyRects;
  }
  if (typeof Range.prototype.getBoundingClientRect !== 'function') {
    Range.prototype.getBoundingClientRect = zeroRect;
  }
  if (typeof Element.prototype.getClientRects !== 'function') {
    Element.prototype.getClientRects = emptyRects;
  }
}
