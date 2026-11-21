import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { createRef, useState, type RefObject } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../../i18n/setup';
import { SAMPLE_ERRORS, SAMPLE_STEPS, STEP_CATALOGUE, STEP_CONTEXT } from './fixtures';
import { StepList } from './StepList';
import type { RuleStep, StepError, StepListHandle, StepRemoval } from './types';

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

interface HarnessProps {
  initial?: readonly RuleStep[];
  errors?: readonly StepError[];
  maxSteps?: number;
  readOnly?: boolean;
  onChange?: (steps: RuleStep[]) => void;
  onStepRemoved?: (removal: StepRemoval) => void;
  handle?: RefObject<StepListHandle | null>;
}

function Harness({
  initial = SAMPLE_STEPS,
  errors,
  maxSteps,
  readOnly,
  onChange,
  onStepRemoved,
  handle,
}: HarnessProps) {
  const [steps, setSteps] = useState<RuleStep[]>(() => initial.map((step) => structuredClone(step)));
  return (
    <I18nextProvider i18n={createI18nInstance()}>
      <StepList
        ref={handle}
        steps={steps}
        catalogue={STEP_CATALOGUE}
        context={STEP_CONTEXT}
        errors={errors}
        maxSteps={maxSteps}
        readOnly={readOnly}
        onStepRemoved={onStepRemoved}
        onChange={(next) => {
          onChange?.(next);
          setSteps(next);
        }}
      />
    </I18nextProvider>
  );
}

function titles(): string[] {
  return screen.getAllByRole('region').map((region) => region.getAttribute('aria-label') ?? '');
}

describe('StepList cards', () => {
  it('names each card by position and shows summary, guard line and phase badge collapsed', () => {
    render(<Harness />);

    expect(titles()).toEqual(['Step 1 of 3: Extract value', 'Step 2 of 3: Respond', 'Step 3 of 3: Send request']);
    const extract = screen.getByRole('region', { name: 'Step 1 of 3: Extract value' });
    const respond = screen.getByRole('region', { name: 'Step 2 of 3: Respond' });
    const http = screen.getByRole('region', { name: 'Step 3 of 3: Send request' });

    expect(within(extract).getByText('from request.body into amount')).toBeDefined();
    expect(within(extract).queryByText('Before response')).toBeNull();
    expect(within(respond).getByText('Before response')).toBeDefined();
    expect(within(http).getByText('After response')).toBeDefined();
    expect(within(http).getByText('only if var.amount is greater than 0')).toBeDefined();
  });

  it('expands from the heading button and shows the fields and the Produces line', () => {
    render(<Harness />);
    const extract = screen.getByRole('region', { name: 'Step 1 of 3: Extract value' });
    const heading = within(extract).getByRole('button', { name: /^\d\s*Extract value$/ });

    expect(heading.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(heading);

    expect(heading.getAttribute('aria-expanded')).toBe('true');
    expect(within(extract).getByRole('textbox', { name: /^From/ })).toBeDefined();
    expect(within(extract).getByText('Produces')).toBeDefined();
    expect(within(extract).getByText('var.amount')).toBeDefined();
  });

  it('collapses on Escape and returns focus to the heading', () => {
    render(<Harness />);
    const extract = screen.getByRole('region', { name: 'Step 1 of 3: Extract value' });
    const heading = within(extract).getByRole('button', { name: /^\d\s*Extract value$/ });
    fireEvent.click(heading);

    fireEvent.keyDown(within(extract).getByRole('textbox', { name: /^From/ }), { key: 'Escape' });

    expect(heading.getAttribute('aria-expanded')).toBe('false');
    return waitFor(() => {
      expect(document.activeElement).toBe(heading);
    });
  });

  it('shows the guard editor and keeps an enabled empty guard as an error', () => {
    render(<Harness />);
    const respond = screen.getByRole('region', { name: 'Step 2 of 3: Respond' });
    fireEvent.click(within(respond).getByRole('button', { name: /^\d\s*Respond$/ }));

    fireEvent.click(within(respond).getByRole('checkbox', { name: 'Run this step only if a value matches' }));

    expect(within(respond).getByText('1 error')).toBeDefined();
    expect(within(respond).getByText('Fill in the value and what to compare it with.')).toBeDefined();
  });

  it('reports a duplicate step name on the card', () => {
    const steps: RuleStep[] = SAMPLE_STEPS.map((step) => ({ ...structuredClone(step), name: 'crm' }));
    render(<Harness initial={steps} />);

    expect(screen.getAllByText('1 error')).toHaveLength(3);
  });
});

describe('StepList errors', () => {
  it('shows server errors on their field and step-level errors on the card', () => {
    render(<Harness errors={SAMPLE_ERRORS} />);
    const http = screen.getByRole('region', { name: 'Step 3 of 3: Send request' });
    const respond = screen.getByRole('region', { name: 'Step 2 of 3: Respond' });

    expect(within(http).getByText('1 error')).toBeDefined();
    expect(respond.getAttribute('aria-describedby')).not.toBeNull();
    expect(within(respond).getByText('A response step cannot come after a step that sends a request.')).toBeDefined();

    fireEvent.click(within(http).getByRole('button', { name: /^\d\s*Send request$/ }));
    expect(within(http).getByText('The address host is not allowed on this plan.')).toBeDefined();
  });

  it('expands a step and focuses a field through the handle', async () => {
    const handle = createRef<StepListHandle>();
    render(<Harness handle={handle} />);

    act(() => {
      handle.current?.focusStep('sample-extract', 'into');
    });

    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole('textbox', { name: /^Into variable/ }));
    });
  });
});

describe('StepList editing', () => {
  it('adds a step between two others, expanded, with its first field focused', async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Add step at position 2' }));
    const filter = screen.getByRole('combobox', { name: 'Filter step types' });
    fireEvent.change(filter, { target: { value: 'increment' } });
    fireEvent.keyDown(filter, { key: 'Enter' });

    const kinds = (onChange.mock.calls.at(-1)?.[0] as RuleStep[]).map((step) => step.kind);
    expect(kinds).toEqual(['extract', 'increment', 'respond', 'http']);
    expect(titles()[1]).toBe('Step 2 of 4: Increment counter');
    expect(screen.getByText('Increment counter added at position 2 of 4')).toBeDefined();
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole('textbox', { name: /^Counter/ }));
    });
  });

  it('groups the menu by class and filters by title', () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole('button', { name: 'Add step at position 4' }));

    const list = screen.getByRole('listbox', { name: 'Step types' });
    expect(
      within(list)
        .getAllByRole('group')
        .map((group) => group.getAttribute('aria-labelledby') !== null),
    ).toEqual([true, true, true, true, true]);
    expect(within(list).getByText('Side effects')).toBeDefined();

    fireEvent.change(screen.getByRole('combobox', { name: 'Filter step types' }), { target: { value: 'zzz' } });
    expect(screen.getByText('No step type matches the filter.')).toBeDefined();
  });

  it('moves a step with the button and announces the new position', () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Move step 1 down' }));

    expect(titles()).toEqual(['Step 1 of 3: Respond', 'Step 2 of 3: Extract value', 'Step 3 of 3: Send request']);
    expect(screen.getByText('Extract value moved to position 2 of 3')).toBeDefined();
  });

  it('moves a step with Alt+Shift+Arrow from its heading and ignores the ends', () => {
    render(<Harness />);
    const heading = within(screen.getByRole('region', { name: 'Step 3 of 3: Send request' })).getByRole('button', {
      name: /^\d\s*Send request$/,
    });

    fireEvent.keyDown(heading, { key: 'ArrowDown', altKey: true, shiftKey: true });
    expect(titles()[2]).toBe('Step 3 of 3: Send request');

    fireEvent.keyDown(heading, { key: 'ArrowUp', altKey: true, shiftKey: true });
    expect(titles()).toEqual(['Step 1 of 3: Extract value', 'Step 2 of 3: Send request', 'Step 3 of 3: Respond']);
    expect(screen.getByText('Send request moved to position 2 of 3')).toBeDefined();
  });

  it('removes a step with Delete on its heading and reports it for undo', () => {
    const onStepRemoved = vi.fn();
    render(<Harness onStepRemoved={onStepRemoved} />);
    const heading = within(screen.getByRole('region', { name: 'Step 2 of 3: Respond' })).getByRole('button', {
      name: /^\d\s*Respond$/,
    });

    fireEvent.keyDown(heading, { key: 'Delete' });

    expect(titles()).toEqual(['Step 1 of 2: Extract value', 'Step 2 of 2: Send request']);
    expect(onStepRemoved).toHaveBeenCalledTimes(1);
    expect(onStepRemoved.mock.calls[0]?.[0]).toMatchObject({ index: 1, step: { id: 'sample-respond' } });
  });

  it('does not delete a step when Delete is pressed inside a field', () => {
    render(<Harness />);
    const extract = screen.getByRole('region', { name: 'Step 1 of 3: Extract value' });
    fireEvent.click(within(extract).getByRole('button', { name: /^\d\s*Extract value$/ }));

    fireEvent.keyDown(within(extract).getByRole('textbox', { name: /^From/ }), { key: 'Delete' });

    expect(titles()).toHaveLength(3);
  });

  it('removes the last step and shows the empty state with the add button', () => {
    render(<Harness initial={[SAMPLE_STEPS[0] as RuleStep]} />);

    fireEvent.click(screen.getByRole('button', { name: 'Remove step 1: Extract value' }));

    expect(screen.getByText('No steps yet. A rule needs at least one step.')).toBeDefined();
    expect(screen.getByRole('button', { name: 'Add step at position 1' })).toBeDefined();
  });
});

describe('StepList limits and read-only', () => {
  it('disables every add button at the step limit', () => {
    render(<Harness maxSteps={3} />);

    for (const button of screen.getAllByRole('button', { name: /^Add step at position/ })) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
    }
    expect(screen.getByText(/Step limit reached\. 3 of 3 steps/)).toBeDefined();
  });

  it('offers no add, move or remove controls when read only', () => {
    render(<Harness readOnly />);

    expect(screen.queryByRole('button', { name: /^Add step/ })).toBeNull();
    expect(screen.queryByRole('button', { name: /^Move step/ })).toBeNull();
    expect(screen.queryByRole('button', { name: /^Remove step/ })).toBeNull();
    expect(titles()).toHaveLength(3);
  });
});
