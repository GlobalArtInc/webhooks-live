import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { VARIABLE_CATALOGUE } from '../../app/editorsSampleData';
import { createI18nInstance } from '../../i18n/setup';
import { VariablePicker } from './VariablePicker';
import { filterVariables, groupVariables, variableCompletionSource } from './variables';

class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

beforeAll(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
});

afterEach(() => {
  cleanup();
});

function renderPicker(onInsert = vi.fn(), loading = false) {
  render(
    <I18nextProvider i18n={createI18nInstance()}>
      <VariablePicker
        catalogue={VARIABLE_CATALOGUE}
        onInsert={onInsert}
        loading={loading}
        sampleCapture="POST /stripe"
      />
    </I18nextProvider>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Insert variable' }));
  return onInsert;
}

describe('variable helpers', () => {
  it('filters by path or description, ignoring case', () => {
    expect(filterVariables(VARIABLE_CATALOGUE, 'AMOUNT').map((entry) => entry.path)).toEqual([
      'json.data.amount',
      'vars.amount',
    ]);
    expect(filterVariables(VARIABLE_CATALOGUE, 'random uuid').map((entry) => entry.path)).toEqual(['uuid']);
    expect(filterVariables(VARIABLE_CATALOGUE, '  ')).toHaveLength(VARIABLE_CATALOGUE.length);
  });

  it('groups entries in first-seen order', () => {
    expect(groupVariables(VARIABLE_CATALOGUE).map((group) => group.id)).toEqual([
      'request',
      'time',
      'rules',
      'helpers',
      'extras',
    ]);
  });

  it('offers the bare path inside a template and the catalogue text outside', async () => {
    const source = variableCompletionSource(VARIABLE_CATALOGUE);
    const inside = await source({ prefix: '', insideTemplate: true, explicit: false });
    const outside = await source({ prefix: '', insideTemplate: false, explicit: true });

    expect(inside[4]?.insertText).toBe('json.id');
    expect(outside[4]?.insertText).toBe('{{ request.json.id }}');
  });
});

describe('VariablePicker', () => {
  it('lists groups, filters by typing and inserts the active entry on Enter', () => {
    const onInsert = renderPicker();

    expect(screen.getByRole('group', { name: 'Request' })).toBeTruthy();
    expect(screen.getByRole('group', { name: 'extras' })).toBeTruthy();
    const filter = screen.getByRole('combobox', { name: 'Filter variables' });
    fireEvent.change(filter, { target: { value: 'amount' } });
    expect(screen.getAllByRole('option')).toHaveLength(2);
    fireEvent.keyDown(filter, { key: 'ArrowDown' });
    fireEvent.keyDown(filter, { key: 'Enter' });

    expect(onInsert).toHaveBeenCalledTimes(1);
    expect(onInsert.mock.calls[0]?.[0]).toMatchObject({ path: 'vars.amount' });
  });

  it('inserts a clicked option and closes', () => {
    const onInsert = renderPicker();

    fireEvent.click(screen.getByRole('option', { name: /^uuid/ }));

    expect(onInsert.mock.calls[0]?.[0]).toMatchObject({ path: 'uuid' });
    expect(screen.queryByRole('listbox')).toBeNull();
  });

  it('says so when nothing matches, and when the catalogue is loading', () => {
    renderPicker();
    fireEvent.change(screen.getByRole('combobox', { name: 'Filter variables' }), { target: { value: 'zzz' } });
    expect(screen.getByRole('status').textContent).toBe('No variables match the filter.');
    cleanup();

    renderPicker(vi.fn(), true);
    expect(screen.getByRole('status').textContent).toBe('Loading variables');
    expect(screen.getByRole('listbox').getAttribute('aria-busy')).toBe('true');
  });
});
