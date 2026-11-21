import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SAMPLE_CAPTURES, SAMPLE_NOW_MS } from '../../app/editorsSampleData';
import { createI18nInstance } from '../../i18n/setup';
import { CapturePicker, type CapturePickerProps } from './CapturePicker';

function renderPicker(overrides: Partial<CapturePickerProps> = {}) {
  const props: CapturePickerProps = {
    captures: SAMPLE_CAPTURES,
    value: { mode: 'latest' },
    onChange: vi.fn(),
    onSendSample: vi.fn(),
    now: SAMPLE_NOW_MS,
    ...overrides,
  };
  render(
    <I18nextProvider i18n={createI18nInstance()}>
      <CapturePicker {...props} />
    </I18nextProvider>,
  );
  return props;
}

afterEach(() => {
  cleanup();
});

describe('CapturePicker', () => {
  it('follows the latest capture and pins it on Pin', () => {
    const props = renderPicker();

    expect(screen.getByRole('status').textContent).toContain('Following the latest capture: POST /stripe');
    fireEvent.click(screen.getByRole('button', { name: 'Pin' }));

    expect(props.onChange).toHaveBeenCalledWith({ mode: 'pinned', captureId: SAMPLE_CAPTURES[0]?.id });
  });

  it('shows the pinned state and returns to following', () => {
    const props = renderPicker({ value: { mode: 'pinned', captureId: SAMPLE_CAPTURES[1]?.id ?? '' } });

    expect(screen.getByRole('status').textContent).toBe('Pinned to one capture');
    expect((screen.getByRole('combobox') as HTMLSelectElement).value).toBe(SAMPLE_CAPTURES[1]?.id);
    fireEvent.click(screen.getByRole('button', { name: 'Follow latest' }));

    expect(props.onChange).toHaveBeenCalledWith({ mode: 'latest' });
  });

  it('pins the capture chosen in the list', () => {
    const props = renderPicker();

    fireEvent.change(screen.getByRole('combobox'), { target: { value: SAMPLE_CAPTURES[2]?.id } });

    expect(props.onChange).toHaveBeenCalledWith({ mode: 'pinned', captureId: SAMPLE_CAPTURES[2]?.id });
  });

  it('keeps a pinned capture that fell out of the recent list selectable', () => {
    renderPicker({ value: { mode: 'pinned', captureId: 'older-capture' } });

    expect((screen.getByRole('combobox') as HTMLSelectElement).value).toBe('older-capture');
  });

  it('offers the sample request on an empty endpoint', () => {
    const props = renderPicker({ captures: [] });

    expect(screen.getByText('This endpoint has no captures yet.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Send a test request to this endpoint' }));

    expect(props.onSendSample).toHaveBeenCalledTimes(1);
  });

  it('disables the sample button while it is sending', () => {
    renderPicker({ captures: [], sendingSample: true });

    expect((screen.getByRole('button', { name: 'Sending a test request' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('disables the controls while loading', () => {
    renderPicker({ captures: undefined });

    expect((screen.getByRole('combobox') as HTMLSelectElement).disabled).toBe(true);
    expect(screen.getByRole('status').textContent).toBe('Loading captures');
  });
});
