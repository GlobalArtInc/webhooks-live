import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { KeyValueEditor, type KeyValueEditorProps } from './KeyValueEditor';
import { createKeyValueRow, findLengthFailures, moveRow, removeRow, type KeyValueRow } from './rows';

interface HarnessProps extends Omit<KeyValueEditorProps, 'rows' | 'onChange'> {
  initial: KeyValueRow[];
  spy?: (rows: KeyValueRow[]) => void;
}

function Harness({ initial, spy, ...props }: HarnessProps) {
  const [rows, setRows] = useState(initial);
  return (
    <I18nextProvider i18n={createI18nInstance()}>
      <KeyValueEditor
        {...props}
        rows={rows}
        onChange={(next) => {
          spy?.(next);
          setRows(next);
        }}
      />
    </I18nextProvider>
  );
}

const THREE: KeyValueRow[] = [
  { id: 'a', name: 'alpha', value: '1' },
  { id: 'b', name: 'beta', value: '2' },
  { id: 'c', name: 'gamma', value: '3' },
];

afterEach(() => {
  cleanup();
});

describe('row helpers', () => {
  it('moves a row and ignores moves past either end', () => {
    expect(moveRow(THREE, 0, 1).map((row) => row.id)).toEqual(['b', 'a', 'c']);
    expect(moveRow(THREE, 0, -1).map((row) => row.id)).toEqual(['a', 'b', 'c']);
    expect(moveRow(THREE, 2, 1).map((row) => row.id)).toEqual(['a', 'b', 'c']);
  });

  it('removes a row by position and creates rows with distinct ids', () => {
    expect(removeRow(THREE, 1).map((row) => row.id)).toEqual(['a', 'c']);
    expect(createKeyValueRow().id).not.toBe(createKeyValueRow().id);
  });

  it('reports rows over the length limits', () => {
    const failures = findLengthFailures([{ id: 'x', name: 'toolong', value: 'ok' }], {
      maxNameLength: 3,
      maxValueLength: 5,
    });
    expect(failures).toEqual([{ rowId: 'x', field: 'name', max: 3 }]);
  });
});

describe('KeyValueEditor', () => {
  it('adds a row and focuses its name field', () => {
    render(<Harness label="Headers" initial={[]} />);

    expect(screen.getByText('No rows yet.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Add row' }));

    expect(document.activeElement).toBe(screen.getByRole('textbox', { name: 'Row 1: Name' }));
  });

  it('edits a value and reports the new rows', () => {
    const spy = vi.fn();
    render(<Harness label="Headers" initial={THREE} spy={spy} />);

    fireEvent.change(screen.getByRole('textbox', { name: 'Row 2: Value' }), { target: { value: 'changed' } });

    const reported = spy.mock.calls.at(-1)?.[0] as KeyValueRow[];
    expect(reported.map((row) => row.value)).toEqual(['1', 'changed', '3']);
  });

  it('moves a row with the buttons and with Alt+Shift+Arrow, announcing the position', () => {
    render(<Harness label="Headers" initial={THREE} />);

    fireEvent.click(screen.getByRole('button', { name: 'Move row 1 down' }));
    expect((screen.getByRole('textbox', { name: 'Row 2: Name' }) as HTMLInputElement).value).toBe('alpha');

    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Row 2: Name' }), {
      key: 'ArrowDown',
      altKey: true,
      shiftKey: true,
    });
    expect((screen.getByRole('textbox', { name: 'Row 3: Name' }) as HTMLInputElement).value).toBe('alpha');
    expect(screen.getByText('Row moved to position 3 of 3')).toBeTruthy();
  });

  it('removes a row and moves focus to its neighbour', () => {
    render(<Harness label="Headers" initial={THREE} />);

    fireEvent.click(screen.getByRole('button', { name: 'Remove row 1' }));

    expect(screen.queryByDisplayValue('alpha')).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('textbox', { name: 'Row 1: Name' }));
  });

  it('disables adding at the row limit and says so', () => {
    render(<Harness label="Headers" initial={THREE} maxRows={3} />);

    expect((screen.getByRole('button', { name: 'Add row' }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText('Row limit reached. 3 of 3 rows')).toBeTruthy();
  });

  it('links a per-row error to its field and flags an over-long name', () => {
    render(
      <Harness
        label="Headers"
        initial={THREE}
        maxNameLength={4}
        errors={[{ rowId: 'b', field: 'value', message: 'Value rejected' }]}
      />,
    );

    const value = screen.getByRole('textbox', { name: 'Row 2: Value' });
    expect(value.getAttribute('aria-invalid')).toBe('true');
    expect(value.getAttribute('aria-describedby')).toBe(screen.getByText('Value rejected').id);
    expect(screen.getByRole('textbox', { name: 'Row 1: Name' }).getAttribute('aria-invalid')).toBe('true');
    expect(screen.getAllByText('The name is longer than 4 characters.')).toHaveLength(2);
  });

  it('shows no controls that change data in read-only mode', () => {
    render(<Harness label="Headers" initial={THREE} readOnly />);

    expect(screen.queryByRole('button', { name: 'Add row' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Remove row 1' })).toBeNull();
    expect((screen.getByRole('textbox', { name: 'Row 1: Name' }) as HTMLInputElement).readOnly).toBe(true);
  });
});
