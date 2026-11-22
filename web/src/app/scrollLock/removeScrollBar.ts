import { useEffect } from 'react';

export const zeroRightClassName = 'right-scroll-bar-position';
export const fullWidthClassName = 'width-before-scroll-bar';
export const noScrollbarsClassName = 'with-scroll-bars-hidden';
export const removedBarSizeVariable = '--removed-body-scroll-bar-size';

const LOCK_ATTRIBUTE = 'data-scroll-locked';

function lockCount(body: HTMLElement): number {
  const count = Number.parseInt(body.getAttribute(LOCK_ATTRIBUTE) ?? '0', 10);
  return Number.isFinite(count) ? count : 0;
}

function scrollbarWidth(): number {
  return Math.max(0, window.innerWidth - document.documentElement.clientWidth);
}

export function RemoveScrollBar(): null {
  useEffect(() => {
    const body = document.body;
    const held = lockCount(body);
    if (held === 0) {
      body.style.setProperty(removedBarSizeVariable, `${scrollbarWidth()}px`);
    }
    body.setAttribute(LOCK_ATTRIBUTE, String(held + 1));
    return () => {
      const remaining = lockCount(body) - 1;
      if (remaining > 0) {
        body.setAttribute(LOCK_ATTRIBUTE, String(remaining));
        return;
      }
      body.removeAttribute(LOCK_ATTRIBUTE);
      body.style.removeProperty(removedBarSizeVariable);
    };
  }, []);
  return null;
}
