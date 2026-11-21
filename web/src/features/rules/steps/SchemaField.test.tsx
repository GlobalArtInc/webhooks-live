import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../../i18n/setup';
import { STEP_CATALOGUE, STEP_CONTEXT } from './fixtures';
import { SchemaField } from './SchemaField';
import type { FieldKind, FieldValue, StepFieldSchema } from './types';

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

function fieldOf(kind: string, name: string): StepFieldSchema {
  const field = STEP_CATALOGUE.find((entry) => entry.kind === kind)?.fields.find((item) => item.name === name);
  if (field === undefined) {
    throw new Error(`fixture field ${kind}.${name} is missing`);
  }
  return field;
}

interface HarnessProps {
  field: StepFieldSchema;
  initial: FieldValue;
  spy: (value: FieldValue) => void;
  error?: string | null;
}

function Harness({ field, initial, spy, error }: HarnessProps) {
  const [value, setValue] = useState<FieldValue>(initial);
  return (
    <I18nextProvider i18n={createI18nInstance()}>
      <SchemaField
        field={field}
        value={value}
        idPrefix="test"
        context={STEP_CONTEXT}
        error={error}
        onChange={(next) => {
          spy(next);
          setValue(next);
        }}
      />
    </I18nextProvider>
  );
}

const ALL_KINDS: readonly FieldKind[] = [
  'text',
  'template',
  'number',
  'duration',
  'boolean',
  'select',
  'key_value',
  'json_path',
  'header_name',
  'channel',
  'secret',
  'code',
];

describe('SchemaField kinds', () => {
  it.each(ALL_KINDS)('renders a %s field from the fixture schema', (kind) => {
    const field = STEP_CATALOGUE.flatMap((entry) => entry.fields).find((item) => item.kind === kind);
    expect(field).toBeDefined();
    if (field === undefined) {
      return;
    }
    const { container } = render(<Harness field={field} initial={''} spy={vi.fn()} />);

    expect(container.querySelector(`[data-kind="${kind}"]`)).not.toBeNull();
    expect(screen.getAllByText(field.label).length).toBeGreaterThan(0);
  });
});

describe('SchemaField behaviour', () => {
  it('emits text and turns a cleared nullable field into null', () => {
    const spy = vi.fn();
    render(<Harness field={fieldOf('extract', 'regex')} initial="abc" spy={spy} />);
    const input = screen.getByRole('textbox', { name: 'Regular expression' });

    fireEvent.change(input, { target: { value: 'abcd' } });
    fireEvent.change(input, { target: { value: '' } });

    expect(spy).toHaveBeenNthCalledWith(1, 'abcd');
    expect(spy).toHaveBeenNthCalledWith(2, null);
  });

  it('emits numbers and null for an empty number input', () => {
    const spy = vi.fn();
    render(<Harness field={fieldOf('delay', 'ms')} initial={500} spy={spy} />);
    const input = screen.getByRole('spinbutton', { name: /Delay/ });

    fireEvent.change(input, { target: { value: '1500' } });
    expect(spy).toHaveBeenLastCalledWith(1500);
    expect(screen.getByText(/about 1.5/)).toBeDefined();

    fireEvent.change(input, { target: { value: '' } });
    expect(spy).toHaveBeenLastCalledWith(null);
  });

  it('marks required fields in text and links the error to the control', () => {
    render(<Harness field={fieldOf('extract', 'from')} initial="" spy={vi.fn()} error="Not a variable path." />);
    const input = screen.getByRole('textbox', { name: /From/ });

    expect(screen.getByText('required')).toBeDefined();
    expect(input.getAttribute('aria-invalid')).toBe('true');
    const describedBy = input.getAttribute('aria-describedby') ?? '';
    expect(document.getElementById(describedBy)?.textContent).toBe('Not a variable path.');
  });

  it('emits the chosen select option and shows a value that left the options', () => {
    const spy = vi.fn();
    render(<Harness field={fieldOf('http', 'method')} initial="TRACE" spy={spy} />);
    const select = screen.getByRole('combobox', { name: /Method/ });

    expect(screen.getByRole('option', { name: 'TRACE (not available)' })).toBeDefined();
    fireEvent.change(select, { target: { value: 'PUT' } });
    expect(spy).toHaveBeenLastCalledWith('PUT');
  });

  it('offers only secret names and says the value is never stored', () => {
    const spy = vi.fn();
    render(<Harness field={fieldOf('store_s3', 'access_key_secret')} initial="" spy={spy} />);
    const select = screen.getByRole('combobox', { name: /Access key secret/ });

    expect(screen.getAllByRole('option').map((option) => option.textContent)).toEqual([
      'Choose a secret',
      'CRM_TOKEN',
      'ARCHIVE_ACCESS',
      'ARCHIVE_SIGNING',
    ]);
    expect(screen.getByText(/never its value/)).toBeDefined();
    fireEvent.change(select, { target: { value: 'ARCHIVE_ACCESS' } });
    expect(spy).toHaveBeenLastCalledWith('ARCHIVE_ACCESS');
  });

  it('lists channels by label and stores the id', () => {
    const spy = vi.fn();
    render(<Harness field={fieldOf('notify', 'channel_id')} initial="" spy={spy} />);

    fireEvent.change(screen.getByRole('combobox', { name: /Channel/ }), { target: { value: 'channel-mail' } });
    expect(spy).toHaveBeenLastCalledWith('channel-mail');
    expect(screen.getByRole('option', { name: 'Team mailbox' })).toBeDefined();
  });

  it('toggles a boolean', () => {
    const spy = vi.fn();
    render(<Harness field={fieldOf('http', 'follow_redirects')} initial={true} spy={spy} />);

    fireEvent.click(screen.getByRole('checkbox', { name: 'Follow redirects' }));
    expect(spy).toHaveBeenLastCalledWith(false);
  });

  it('adds a key-value row and flags a bad header name on it', () => {
    const spy = vi.fn();
    render(<Harness field={fieldOf('respond', 'headers')} initial={[{ name: 'X Bad', value: '1' }]} spy={spy} />);

    expect(screen.getByText(/Header names use letters/)).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'Add row' }));
    expect(spy).toHaveBeenLastCalledWith([
      { name: 'X Bad', value: '1' },
      { name: '', value: '' },
    ]);
  });

  it('gives every template field a labelled variable picker', () => {
    render(<Harness field={fieldOf('set_var', 'value')} initial="" spy={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Insert variable into Value' })).toBeDefined();
  });
});
