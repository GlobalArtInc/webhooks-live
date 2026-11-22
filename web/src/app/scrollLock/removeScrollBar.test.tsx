import { render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { RemoveScrollBar, removedBarSizeVariable } from './removeScrollBar';

const SCROLLBAR_PX = 15;

function narrowViewport(): void {
  Object.defineProperty(document.documentElement, 'clientWidth', {
    value: window.innerWidth - SCROLLBAR_PX,
    configurable: true,
  });
}

afterEach(() => {
  Reflect.deleteProperty(document.documentElement, 'clientWidth');
});

describe('RemoveScrollBar', () => {
  it('locks the body with an attribute and the measured scrollbar width, without any style element', () => {
    narrowViewport();
    const styleElements = document.querySelectorAll('style').length;
    const view = render(<RemoveScrollBar />);

    expect(document.body.getAttribute('data-scroll-locked')).toBe('1');
    expect(document.body.style.getPropertyValue(removedBarSizeVariable)).toBe(`${SCROLLBAR_PX}px`);
    expect(document.querySelectorAll('style')).toHaveLength(styleElements);

    view.unmount();
    expect(document.body.hasAttribute('data-scroll-locked')).toBe(false);
    expect(document.body.style.getPropertyValue(removedBarSizeVariable)).toBe('');
  });

  it('counts nested locks and releases the body only after the last one', () => {
    narrowViewport();
    const outer = render(<RemoveScrollBar />);
    const inner = render(<RemoveScrollBar />);
    expect(document.body.getAttribute('data-scroll-locked')).toBe('2');

    inner.unmount();
    expect(document.body.getAttribute('data-scroll-locked')).toBe('1');
    expect(document.body.style.getPropertyValue(removedBarSizeVariable)).toBe(`${SCROLLBAR_PX}px`);

    outer.unmount();
    expect(document.body.hasAttribute('data-scroll-locked')).toBe(false);
  });
});
