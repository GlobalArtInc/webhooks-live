import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { KeyValueTable, type KeyValueTableProps } from './KeyValueTable';

function renderTable(props: KeyValueTableProps) {
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <KeyValueTable {...props} />
    </I18nextProvider>,
  );
}

afterEach(() => {
  cleanup();
});

describe('KeyValueTable', () => {
  it('switches between decoded and raw rows with the raw toggle', () => {
    renderTable({
      label: 'Query',
      rows: [{ name: 'a b', value: '1' }],
      rawRows: [{ name: 'a%20b', value: '1' }],
    });
    const toggle = screen.getByRole('button', { name: 'Raw' });

    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    expect(screen.getByRole('rowheader', { name: 'a b' })).toBeTruthy();

    fireEvent.click(toggle);

    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('rowheader', { name: 'a%20b' })).toBeTruthy();
  });

  it('shows the first rows, then all rows on Show all, then fewer again', () => {
    const rows = Array.from({ length: 14 }, (_, index) => ({ name: `x-${index}`, value: 'v' }));
    renderTable({ label: 'Headers', rows, collapseAfter: 12 });

    expect(screen.getAllByRole('row')).toHaveLength(13);

    fireEvent.click(screen.getByRole('button', { name: 'Show all 14' }));
    expect(screen.getAllByRole('row')).toHaveLength(15);

    fireEvent.click(screen.getByRole('button', { name: 'Show fewer' }));
    expect(screen.getAllByRole('row')).toHaveLength(13);
  });

  it('renders the empty state without a table when there are no rows', () => {
    renderTable({ label: 'Query', rows: [] });

    expect(screen.getByText('None')).toBeTruthy();
    expect(screen.queryByRole('table')).toBeNull();
  });
});
