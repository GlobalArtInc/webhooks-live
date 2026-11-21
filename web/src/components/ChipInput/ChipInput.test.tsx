import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { ChipInput, type ChipInputProps } from './ChipInput';
import { addChips, splitChips } from './chips';

interface HarnessProps extends Omit<ChipInputProps, 'values' | 'onChange'> {
  initial: string[];
  spy?: (values: string[]) => void;
}

function Harness({ initial, spy, ...props }: HarnessProps) {
  const [values, setValues] = useState(initial);
  return (
    <I18nextProvider i18n={createI18nInstance()}>
      <ChipInput
        {...props}
        values={values}
        onChange={(next) => {
          spy?.(next);
          setValues(next);
        }}
      />
    </I18nextProvider>
  );
}

function rejectBang(value: string): string | null {
  return value.includes('!') ? `${value} has a bang` : null;
}

afterEach(() => {
  cleanup();
});

describe('chip helpers', () => {
  it('splits on commas and line breaks and drops blanks', () => {
    expect(splitChips(' a, b\nc ,, ')).toEqual(['a', 'b', 'c']);
  });

  it('stops at the first failing piece and returns it with the rest', () => {
    const batch = addChips(['a'], ['b', 'a', 'c'], { allowDuplicates: false });

    expect(batch.accepted).toEqual(['b']);
    expect(batch.failure?.kind).toBe('duplicate');
    expect(batch.rest).toEqual(['a', 'c']);
  });

  it('applies the limit, the length bound and the validator in that order', () => {
    expect(addChips(['a'], ['b'], { allowDuplicates: false, maxItems: 1 }).failure?.kind).toBe('limit');
    expect(addChips([], ['long'], { allowDuplicates: false, maxLength: 3 }).failure?.kind).toBe('tooLong');
    expect(addChips([], ['x!'], { allowDuplicates: false, validate: rejectBang }).failure?.detail).toBe(
      'x! has a bang',
    );
  });
});

describe('ChipInput', () => {
  it('adds a chip on Enter and on a comma, and reports the list', () => {
    const spy = vi.fn();
    render(<Harness label="Senders" initial={[]} spy={spy} />);
    const input = screen.getByRole('textbox', { name: 'Senders' });

    fireEvent.change(input, { target: { value: 'one' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    fireEvent.change(input, { target: { value: 'two' } });
    fireEvent.keyDown(input, { key: ',' });

    expect(spy).toHaveBeenLastCalledWith(['one', 'two']);
    expect(screen.getByRole('button', { name: 'Remove one' })).toBeTruthy();
  });

  it('keeps an invalid entry in the field with an alert and no new chip', () => {
    const spy = vi.fn();
    render(<Harness label="Senders" initial={[]} validate={rejectBang} spy={spy} />);
    const input = screen.getByRole('textbox', { name: 'Senders' }) as HTMLInputElement;

    fireEvent.change(input, { target: { value: 'bad!' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(spy).not.toHaveBeenCalled();
    expect(input.value).toBe('bad!');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(screen.getByRole('alert').textContent).toBe('bad! has a bang');
  });

  it('rejects a duplicate and explains it', () => {
    render(<Harness label="Senders" initial={['one']} />);
    const input = screen.getByRole('textbox', { name: 'Senders' });

    fireEvent.change(input, { target: { value: 'one' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(screen.getByRole('alert').textContent).toBe('one is already in the list.');
  });

  it('removes the last chip with Backspace on an empty field and a chosen chip with its button', () => {
    const spy = vi.fn();
    render(<Harness label="Senders" initial={['a', 'b', 'c']} spy={spy} />);
    const input = screen.getByRole('textbox', { name: 'Senders' });

    fireEvent.keyDown(input, { key: 'Backspace' });
    expect(spy).toHaveBeenLastCalledWith(['a', 'b']);

    fireEvent.click(screen.getByRole('button', { name: 'Remove a' }));
    expect(spy).toHaveBeenLastCalledWith(['b']);
    expect(document.activeElement).toBe(input);
  });

  it('marks server-rejected chips with text as well as colour', () => {
    render(<Harness label="Senders" initial={['a', 'b']} invalidValues={['b']} />);

    expect(screen.getByText('invalid')).toBeTruthy();
  });

  it('stops accepting input at the item limit and in read-only mode', () => {
    const { rerender } = render(<Harness label="Senders" initial={['a']} maxItems={1} />);
    expect((screen.getByRole('textbox', { name: 'Senders' }) as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText('Limit of 1 values reached.')).toBeTruthy();

    rerender(<Harness label="Senders" initial={['a']} readOnly />);
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Remove a' })).toBeNull();
  });
});
