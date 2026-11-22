import { cleanup, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { MiddleTruncate } from './MiddleTruncate';

afterEach(cleanup);

describe('MiddleTruncate', () => {
  it('keeps the whole text readable and in the title, and splits off the tail', () => {
    const { container } = render(<MiddleTruncate text="https://hooks.example/abcdefghijklmnop" tailLength={4} />);

    const root = container.firstElementChild;
    expect(root?.getAttribute('title')).toBe('https://hooks.example/abcdefghijklmnop');
    expect(root?.textContent).toBe('https://hooks.example/abcdefghijklmnop');
    expect(root?.lastElementChild?.textContent).toBe('mnop');
  });

  it('keeps a fixed prefix in front of the shrinking part', () => {
    const { container } = render(<MiddleTruncate prefix="hooks.example/" text="abcdefghijklmnop" tailLength={4} />);

    const root = container.firstElementChild;
    expect(root?.getAttribute('title')).toBe('hooks.example/abcdefghijklmnop');
    expect(root?.textContent).toBe('hooks.example/abcdefghijklmnop');
    expect(root?.firstElementChild?.textContent).toBe('hooks.example/');
  });

  it('puts a text shorter than the tail entirely in the tail', () => {
    const { container } = render(<MiddleTruncate text="abc" tailLength={10} />);

    expect(container.firstElementChild?.firstElementChild?.textContent).toBe('');
    expect(container.firstElementChild?.lastElementChild?.textContent).toBe('abc');
  });
});
