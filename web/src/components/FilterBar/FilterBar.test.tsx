import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CaptureSearch } from '../../features/filters/captureSearch';
import { createI18nInstance } from '../../i18n/setup';
import { FilterBar, SEARCH_DEBOUNCE_MS, type FilterBarProps } from './FilterBar';

function renderBar(overrides: Partial<FilterBarProps> = {}, search: CaptureSearch = {}) {
  const props: FilterBarProps = {
    search,
    layout: 'wide',
    hiddenCount: 0,
    onChange: vi.fn(),
    onClear: vi.fn(),
    ...overrides,
  };
  render(
    <I18nextProvider i18n={createI18nInstance()}>
      <FilterBar {...props} />
    </I18nextProvider>,
  );
  return props;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function openOptions(label = 'Filters (0)'): void {
  fireEvent.click(screen.getByRole('button', { name: label }));
}

describe('FilterBar', () => {
  it('sends the text query only after the typing pause ends', () => {
    const props = renderBar();
    const input = screen.getByRole('searchbox', { name: 'Search captures' });

    fireEvent.change(input, { target: { value: 'inv' } });
    fireEvent.change(input, { target: { value: 'invoice' } });
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS - 1);
    expect(props.onChange).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(props.onChange).toHaveBeenCalledTimes(1);
    expect(props.onChange).toHaveBeenCalledWith({ q: 'invoice' });
  });

  it('toggles a kind chip in and out of the kind list and reports its state', () => {
    const props = renderBar({}, { kind: 'email' });
    openOptions('Filters (1)');
    const email = screen.getByRole('button', { name: 'Email' });

    expect(email.getAttribute('aria-pressed')).toBe('true');
    fireEvent.click(screen.getByRole('button', { name: 'DNS' }));
    expect(props.onChange).toHaveBeenLastCalledWith({ kind: 'email,dns' });
    fireEvent.click(email);
    expect(props.onChange).toHaveBeenLastCalledWith({ kind: undefined });
  });

  it('toggles a method chip in and out of the method list and reports its state', () => {
    const props = renderBar({}, { method: 'GET' });
    openOptions('Filters (1)');
    const get = screen.getByRole('button', { name: 'GET' });
    const post = screen.getByRole('button', { name: 'POST' });

    expect(get.getAttribute('aria-pressed')).toBe('true');
    expect(post.getAttribute('aria-pressed')).toBe('false');

    fireEvent.click(post);
    expect(props.onChange).toHaveBeenLastCalledWith({ method: 'GET,POST' });
    fireEvent.click(get);
    expect(props.onChange).toHaveBeenLastCalledWith({ method: undefined });
  });

  it('keeps the filter options behind a disclosure on medium and narrow layouts', () => {
    renderBar({ layout: 'medium' });
    const toggle = screen.getByRole('button', { name: 'Filters (0)' });

    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('button', { name: 'POST' })).toBeNull();

    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByRole('button', { name: 'POST' })).not.toBeNull();
  });

  it('shows the hidden-by-filter count and clears the filters from there', () => {
    const props = renderBar({ hiddenCount: 2 }, { method: 'GET' });

    expect(screen.getByText('2 new captures hidden by filters')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show all captures' }));
    expect(props.onClear).toHaveBeenCalledTimes(1);
  });
});
