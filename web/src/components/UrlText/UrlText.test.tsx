import { cleanup, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { breakUrl, UrlText } from './UrlText';

afterEach(() => {
  cleanup();
});

describe('breakUrl', () => {
  it('splits after the path, query and parameter separators and keeps every character', () => {
    const url = 'https://webhooks.live/abc/orders?customer=42&state=paid';
    const parts = breakUrl(url);
    expect(parts).toEqual(['https:/', '/', 'webhooks.live/', 'abc/', 'orders?', 'customer=', '42&', 'state=', 'paid']);
    expect(parts.join('')).toBe(url);
  });

  it('leaves a url without separators whole', () => {
    expect(breakUrl('token')).toEqual(['token']);
  });
});

describe('UrlText', () => {
  it('renders the url as text with a break opportunity after each separator', () => {
    const { container } = render(
      <code>
        <UrlText url="https://a.test/x?y=1" />
      </code>,
    );

    expect(container.textContent).toBe('https://a.test/x?y=1');
    expect(container.querySelectorAll('wbr')).toHaveLength(breakUrl('https://a.test/x?y=1').length - 1);
  });
});
