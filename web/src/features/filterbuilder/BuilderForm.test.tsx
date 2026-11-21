import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { BuilderForm } from './BuilderForm';
import { loadSavedFilters } from './savedFilters';

const ENDPOINT_ID = '0123456789abcdefghjkmnpq';

function renderForm(onApply = vi.fn()) {
  render(
    <I18nextProvider i18n={createI18nInstance()}>
      <BuilderForm endpointId={ENDPOINT_ID} search={{}} onApply={onApply} />
    </I18nextProvider>,
  );
  return onApply;
}

beforeEach(() => {
  window.localStorage.clear();
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe('BuilderForm', () => {
  it('applies the chosen method and text as one patch', () => {
    const onApply = renderForm();

    fireEvent.click(screen.getByRole('checkbox', { name: 'POST' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Text' }), { target: { value: 'invoice_id' } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));

    expect(onApply).toHaveBeenCalledWith(expect.objectContaining({ method: 'POST', q: 'invoice_id' }));
  });

  it('shows the sender error and applies nothing for a range wider than 32 bits', () => {
    const onApply = renderForm();

    fireEvent.change(screen.getByRole('textbox', { name: 'Sender address or IPv4 range' }), {
      target: { value: '203.0.113.0/40' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));

    expect(onApply).not.toHaveBeenCalled();
    expect(screen.getByRole('alert').textContent).toContain('Enter an address such as 203.0.113.7');
  });

  it('saves the current filter under its name and lists it for this endpoint only', () => {
    renderForm();

    fireEvent.click(screen.getByRole('checkbox', { name: 'GET' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Name for this filter' }), {
      target: { value: 'Failed GETs' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save filter' }));

    expect(screen.getByRole('button', { name: 'Apply Failed GETs' })).toBeTruthy();
    expect(loadSavedFilters(ENDPOINT_ID)).toEqual([
      expect.objectContaining({ name: 'Failed GETs', filter: 'method=GET' }),
    ]);
  });
});
