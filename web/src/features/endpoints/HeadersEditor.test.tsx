import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { HeadersEditor } from './HeadersEditor';
import { MAX_RESPONSE_HEADERS, NO_ISSUES, type HeaderRow } from './responseDraft';

function ControlledEditor() {
  const [rows, setRows] = useState<readonly HeaderRow[]>([]);
  return <HeadersEditor rows={rows} issues={NO_ISSUES} onChange={setRows} />;
}

function renderEditor() {
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <ControlledEditor />
    </I18nextProvider>,
  );
}

describe('HeadersEditor', () => {
  afterEach(() => {
    cleanup();
  });

  it('adds rows up to the twenty header limit and then disables Add header', () => {
    renderEditor();
    const add = screen.getByRole('button', { name: 'Add header' });

    for (let count = 0; count < MAX_RESPONSE_HEADERS + 1; count += 1) {
      fireEvent.click(add);
    }

    expect(screen.getAllByLabelText(/^Header \d+ name$/)).toHaveLength(MAX_RESPONSE_HEADERS);
    expect(screen.getByRole('button', { name: 'Add header' }).hasAttribute('disabled')).toBe(true);
    expect(screen.getByText('At most 20 headers. Remove one to add another.')).toBeTruthy();
  });

  it('removes a row and makes room to add another', () => {
    renderEditor();
    fireEvent.click(screen.getByRole('button', { name: 'Add header' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add header' }));

    fireEvent.click(screen.getByRole('button', { name: 'Remove header 1' }));

    expect(screen.getAllByLabelText(/^Header \d+ name$/)).toHaveLength(1);
    expect(screen.getByText('1 of 20 headers used')).toBeTruthy();
  });
});
