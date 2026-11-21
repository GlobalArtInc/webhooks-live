import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ReactElement } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { editorCount, editorText } from '../../features/diff/testDom';
import { largeCaptureFixture, leftCaptureFixture, rightCaptureFixture } from '../../features/diff/testFixtures';
import type { DiffLayout } from '../../features/diff/types';
import { DiffView, type DiffViewProps } from './DiffView';

function diffProps(overrides: Partial<DiffViewProps> = {}): DiffViewProps {
  return {
    a: leftCaptureFixture,
    b: rightCaptureFixture,
    hideVolatile: true,
    sortHeaders: false,
    layout: 'split',
    onHideVolatileChange: vi.fn(),
    onSortHeadersChange: vi.fn(),
    onLayoutChange: vi.fn(),
    onSwap: vi.fn(),
    ...overrides,
  };
}

const i18n = createI18nInstance();

function renderDiff(props: DiffViewProps): ReactElement {
  return (
    <I18nextProvider i18n={i18n}>
      <DiffView {...props} />
    </I18nextProvider>
  );
}

describe('DiffView', () => {
  afterEach(() => {
    cleanup();
  });

  it('states the structural summary as text before the editors', () => {
    const { container } = render(renderDiff(diffProps()));

    const summary = screen.getByText('Headers: 1 changed, 2 added, 0 removed, 2 same, 2 hidden as volatile');
    const headingsRegion = screen.getByRole('heading', { level: 2, name: 'Headers' });
    expect(summary.compareDocumentPosition(headingsRegion) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByText('Body: Different')).toBeTruthy();
    expect(container.querySelector('table')).not.toBeNull();
  });

  it('shows the field table with the sides and a text result for each field', () => {
    render(renderDiff(diffProps()));

    const queryRow = screen.getByRole('row', { name: /Query/ });
    expect(within(queryRow).getByText('mode=test')).toBeTruthy();
    expect(within(queryRow).getByText('(empty)')).toBeTruthy();
    expect(within(queryRow).getByText('Different')).toBeTruthy();
    expect(within(screen.getByRole('row', { name: /Method/ })).getByText('Same')).toBeTruthy();
  });

  it('marks the active layout with pressed state and reports a layout change', () => {
    const onLayoutChange = vi.fn<(layout: DiffLayout) => void>();
    render(renderDiff(diffProps({ layout: 'split', onLayoutChange })));

    expect(screen.getByRole('button', { name: 'Side by side' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'Unified' }).getAttribute('aria-pressed')).toBe('false');
    fireEvent.click(screen.getByRole('button', { name: 'Unified' }));
    expect(onLayoutChange).toHaveBeenCalledWith('unified');
  });

  it('reports the next value of the volatile and sort toggles', () => {
    const onHideVolatileChange = vi.fn<(value: boolean) => void>();
    const onSortHeadersChange = vi.fn<(value: boolean) => void>();
    render(renderDiff(diffProps({ hideVolatile: true, onHideVolatileChange, onSortHeadersChange })));

    const hide = screen.getByRole('button', { name: 'Hide volatile headers' });
    expect(hide.getAttribute('aria-pressed')).toBe('true');
    fireEvent.click(hide);
    expect(onHideVolatileChange).toHaveBeenCalledWith(false);

    fireEvent.click(screen.getByRole('button', { name: 'Sort headers' }));
    expect(onSortHeadersChange).toHaveBeenCalledWith(true);
  });

  it('calls onSwap from the swap button', () => {
    const onSwap = vi.fn();
    render(renderDiff(diffProps({ onSwap })));

    fireEvent.click(screen.getByRole('button', { name: 'Swap A and B' }));

    expect(onSwap).toHaveBeenCalledOnce();
  });

  it('hides volatile headers in the headers editor and shows them when asked', async () => {
    const { container, rerender } = render(renderDiff(diffProps({ hideVolatile: true })));

    await waitFor(() => expect(editorText(container)).toContain('X-Retry: 1'));
    expect(editorText(container)).not.toContain('Date: Sat, 21 Nov 2026 13:59:40 GMT');

    rerender(renderDiff(diffProps({ hideVolatile: false })));

    await waitFor(() => expect(editorText(container)).toContain('Date: Sat, 21 Nov 2026 13:59:40 GMT'));
  });

  it('uses two editors side by side and one editor for the unified layout', async () => {
    const { container, rerender } = render(renderDiff(diffProps({ layout: 'split' })));

    await waitFor(() => expect(editorCount(container, '.cm-editor')).toBe(4));

    rerender(renderDiff(diffProps({ layout: 'unified' })));

    await waitFor(() => expect(editorCount(container, '.cm-editor')).toBe(2));
    expect(editorCount(container, '.cm-deletedChunk')).toBeGreaterThan(0);
  });

  it('renders header values that contain markup as text', async () => {
    const { container } = render(renderDiff(diffProps()));

    await waitFor(() => expect(editorText(container)).toContain('<img src=x onerror="alert(1)">'));
    expect(container.querySelector('img')).toBeNull();
  });

  it('does not diff large bodies until the user asks', async () => {
    const { container } = render(renderDiff(diffProps({ a: largeCaptureFixture })));

    expect(screen.getByText(/Body A is 2 MiB and body B is \d+ B/)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Compare anyway' })).toBeTruthy();
    expect(editorText(container)).not.toContain('"amount"');

    fireEvent.click(screen.getByRole('button', { name: 'Compare anyway' }));

    await waitFor(() => expect(editorText(container)).toContain('"amount"'));
  });
});
