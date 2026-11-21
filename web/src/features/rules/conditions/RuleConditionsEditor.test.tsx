import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../../i18n/setup';
import { CONDITION_CATALOGUE, SAMPLE_VARIABLES, UNSUPPORTED_CONDITION, sampleSource } from './fixtures';
import { RuleConditionsEditor } from './RuleConditionsEditor';
import { createSampleServer, type SampleServer } from './sampleServer';
import type { ConditionsInput, RuleConditionsSource } from './types';

vi.mock('../../../features/editor/engine', () => {
  throw new Error('chunk failed');
});

interface HarnessProps {
  server: SampleServer;
  initial?: RuleConditionsSource;
  onSave?: (input: ConditionsInput, version: number) => void;
  onConflict?: () => void;
  onSource?: (push: (source: RuleConditionsSource) => void) => void;
}

function Harness({ server, initial, onSave, onConflict, onSource }: HarnessProps) {
  const [source, setSource] = useState(initial ?? server.read());
  onSource?.(setSource);
  return (
    <I18nextProvider i18n={createI18nInstance()}>
      <RuleConditionsEditor
        source={source}
        catalogue={CONDITION_CATALOGUE}
        variables={SAMPLE_VARIABLES}
        onConflict={onConflict}
        save={async (input, version) => {
          onSave?.(input, version);
          const saved = await server.save(input, version);
          setSource(saved);
          return saved;
        }}
      />
    </I18nextProvider>
  );
}

function row(number: number): HTMLElement {
  return screen.getByRole('group', { name: new RegExp(`^Condition ${number}(?!\\d)`) });
}

function save(): HTMLButtonElement {
  return screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement;
}

async function makeRegexRow(text: string): Promise<void> {
  fireEvent.change(within(row(1)).getByLabelText('Operator'), { target: { value: 'matches' } });
  const field = await within(row(1)).findByRole('textbox', { name: 'Value' });
  fireEvent.change(field, { target: { value: text } });
}

afterEach(() => {
  cleanup();
});

describe('RuleConditionsEditor', () => {
  it('saves the serialised draft with the version it started from and then reads as clean', async () => {
    const server = createSampleServer(sampleSource());
    const onSave = vi.fn();
    render(<Harness server={server} onSave={onSave} />);

    expect(save().disabled).toBe(true);
    fireEvent.change(within(row(1)).getByLabelText('Value'), { target: { value: 'PUT' } });
    fireEvent.click(screen.getByRole('radio', { name: /Stop here/ }));
    fireEvent.click(save());

    await screen.findByText('Conditions saved.');
    const [input, version] = onSave.mock.calls[0] as [ConditionsInput, number];
    expect(version).toBe(7);
    expect(input.on_match).toBe('stop');
    expect(JSON.stringify(input.condition)).toContain('"arg":"PUT"');
    expect(save().disabled).toBe(true);
  });

  it('blocks an unfinished row before any request, shows why and focuses the first problem', async () => {
    const server = createSampleServer(sampleSource());
    const onSave = vi.fn();
    render(<Harness server={server} onSave={onSave} />);

    fireEvent.click(screen.getAllByRole('button', { name: 'Add condition' })[1] as HTMLElement);
    fireEvent.click(save());

    await screen.findByText('Choose a subject.');
    expect(onSave).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(within(row(5)).getByLabelText('Subject'));
    expect(screen.getByText('2 problems in the conditions.')).toBeTruthy();
  });

  it('lands a server field error on its row and focuses the control, and clears it when that row is edited', async () => {
    const server = createSampleServer(sampleSource());
    render(<Harness server={server} />);

    await makeRegexRow('(');
    fireEvent.click(save());

    const message = await within(row(1)).findByText(/regular expression|Invalid|Unterminated|unclosed/i);
    expect(message).toBeTruthy();
    await waitFor(() => {
      expect(document.activeElement).toBe(within(row(1)).getByRole('textbox', { name: 'Value' }));
    });

    fireEvent.change(within(row(1)).getByRole('textbox', { name: 'Value' }), { target: { value: '^POST$' } });
    expect(within(row(1)).queryByText(message.textContent ?? '')).toBeNull();
    fireEvent.click(save());
    await screen.findByText('Conditions saved.');
  });

  it('shows the conflict banner when the rule changes elsewhere while edited, and Reload takes the new version', async () => {
    const server = createSampleServer(sampleSource());
    let push: (source: RuleConditionsSource) => void = () => undefined;
    render(
      <Harness
        server={server}
        onSource={(setter) => {
          push = setter;
        }}
      />,
    );

    fireEvent.change(within(row(1)).getByLabelText('Value'), { target: { value: 'PUT' } });
    act(() => {
      push(server.touch());
    });

    expect(screen.getByRole('alert').textContent).toContain('Changed elsewhere');
    fireEvent.click(screen.getByRole('button', { name: 'Reload' }));

    expect(screen.queryByText(/Changed elsewhere/)).toBeNull();
    expect((within(row(1)).getByLabelText('Value') as HTMLInputElement).value).toBe('POST');
    expect((screen.getByRole('radio', { name: /Stop here/ }) as HTMLInputElement).checked).toBe(true);
  });

  it('keeps the draft on Keep mine and saves it against the new version', async () => {
    const server = createSampleServer(sampleSource());
    const onSave = vi.fn();
    let push: (source: RuleConditionsSource) => void = () => undefined;
    render(
      <Harness
        server={server}
        onSave={onSave}
        onSource={(setter) => {
          push = setter;
        }}
      />,
    );

    fireEvent.change(within(row(1)).getByLabelText('Value'), { target: { value: 'PUT' } });
    act(() => {
      push(server.touch());
    });
    fireEvent.click(screen.getByRole('button', { name: 'Keep mine and overwrite' }));
    fireEvent.click(save());

    await screen.findByText('Conditions saved.');
    expect(onSave.mock.calls[0]?.[1]).toBe(8);
  });

  it('turns a 409 from the server into the conflict banner and asks the parent to refetch', async () => {
    const server = createSampleServer(sampleSource());
    const onConflict = vi.fn();
    render(<Harness server={server} initial={sampleSource({ version: 6 })} onConflict={onConflict} />);

    fireEvent.change(within(row(1)).getByLabelText('Value'), { target: { value: 'PUT' } });
    fireEvent.click(save());

    await screen.findByText(/Changed elsewhere/);
    expect(onConflict).toHaveBeenCalledOnce();
  });

  it('saves on Mod+S from inside a field', async () => {
    const server = createSampleServer(sampleSource());
    render(<Harness server={server} />);

    const value = within(row(1)).getByLabelText('Value');
    fireEvent.change(value, { target: { value: 'PUT' } });
    fireEvent.keyDown(value, { key: 's', ctrlKey: true });

    await screen.findByText('Conditions saved.');
  });

  it('discards the edits after a confirmation', async () => {
    const server = createSampleServer(sampleSource());
    render(<Harness server={server} />);

    fireEvent.change(within(row(1)).getByLabelText('Value'), { target: { value: 'PUT' } });
    fireEvent.click(screen.getByRole('button', { name: 'Discard' }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Discard' }));

    await waitFor(() => {
      expect((within(row(1)).getByLabelText('Value') as HTMLInputElement).value).toBe('POST');
    });
  });

  it('shows a condition it cannot represent as read only instead of rewriting it', () => {
    const server = createSampleServer(sampleSource());
    render(<Harness server={server} initial={sampleSource({ condition: UNSUPPORTED_CONDITION })} />);

    expect(screen.getByRole('alert').textContent).toContain('read only');
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Add condition' })).toBeNull();
  });
});
