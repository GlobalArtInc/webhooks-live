import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { useState } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CONDITION_CATALOGUE, SAMPLE_VARIABLES, sampleSource } from '../../features/rules/conditions/fixtures';
import { newGroup, parseSource } from '../../features/rules/conditions/model';
import type { ConditionLimits, GroupDraft, IssueMap, LeafDraft } from '../../features/rules/conditions/types';
import { validateConditions } from '../../features/rules/conditions/validation';
import { createI18nInstance } from '../../i18n/setup';
import { ConditionGroup } from './ConditionGroup';

vi.mock('../../features/editor/engine', () => {
  throw new Error('chunk failed');
});

interface HarnessProps {
  initial: GroupDraft;
  showIssues?: boolean;
  limits?: ConditionLimits;
  serverIssues?: IssueMap;
  onRoot?: (root: GroupDraft) => void;
}

function sampleRoot(): GroupDraft {
  const parsed = parseSource(sampleSource(), CONDITION_CATALOGUE);
  if (!parsed.ok) {
    throw new Error('sample must parse');
  }
  return parsed.value.root;
}

function Harness({ initial, showIssues = false, limits, serverIssues, onRoot }: HarnessProps) {
  const [root, setRoot] = useState(initial);
  const validation = validateConditions({ triggers: ['http'], root, onMatch: 'next' }, CONDITION_CATALOGUE);
  return (
    <I18nextProvider i18n={createI18nInstance()}>
      <ConditionGroup
        root={root}
        onChange={(next) => {
          onRoot?.(next);
          setRoot(next);
        }}
        catalogue={CONDITION_CATALOGUE}
        triggers={['http']}
        variables={SAMPLE_VARIABLES}
        issues={serverIssues ?? validation.rows}
        showIssues={showIssues}
        limits={limits}
      />
    </I18nextProvider>
  );
}

function firstLeaf(root: GroupDraft): LeafDraft {
  const found = root.children[0];
  if (found === undefined || found.kind !== 'leaf') {
    throw new Error('expected a leaf');
  }
  return found;
}

function row(number: number): HTMLElement {
  return screen.getByRole('group', { name: new RegExp(`^Condition ${number}(?!\\d)`) });
}

afterEach(() => {
  cleanup();
});

describe('ConditionGroup', () => {
  it('names every row and its remove button by number and removes the right one', () => {
    render(<Harness initial={sampleRoot()} />);

    expect(within(row(2)).getByLabelText('Subject')).toHaveProperty('value', 'header');
    fireEvent.click(screen.getByRole('button', { name: 'Remove condition 2' }));

    expect(screen.queryByRole('button', { name: 'Remove condition 4' })).toBeNull();
    expect(within(row(2)).getByLabelText('Subject')).toHaveProperty('value', 'json');
  });

  it('adds a row at the end of a group and focuses its subject', () => {
    render(<Harness initial={newGroup()} />);

    expect(screen.getByText('No conditions: the rule matches every capture of the chosen kinds.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Add condition' }));

    expect(document.activeElement).toBe(within(row(1)).getByLabelText('Subject'));
    expect(within(row(1)).getByText('Incomplete')).toBeTruthy();
  });

  it('offers only the operators the subject allows and swaps an operator it does not allow', () => {
    render(<Harness initial={sampleRoot()} />);
    const subject = within(row(1)).getByLabelText('Subject');
    const operator = within(row(1)).getByLabelText('Operator') as HTMLSelectElement;

    expect(Array.from(operator.options).map((option) => option.value)).not.toContain('gt');
    fireEvent.change(subject, { target: { value: 'amount' } });

    expect(Array.from(operator.options).map((option) => option.value)).toContain('gt');
    fireEvent.change(operator, { target: { value: 'gt' } });
    fireEvent.change(subject, { target: { value: 'path' } });
    expect(operator.value).toBe('eq');
  });

  it('draws the value widget from the operator: none, regex editor and a chip list', () => {
    render(<Harness initial={sampleRoot()} />);
    const operator = within(row(1)).getByLabelText('Operator');

    fireEvent.change(operator, { target: { value: 'exists' } });
    expect(within(row(1)).queryByLabelText('Value')).toBeNull();

    fireEvent.change(operator, { target: { value: 'matches' } });
    expect(within(row(1)).getByRole('group', { name: 'Value' })).toBeTruthy();

    fireEvent.change(operator, { target: { value: 'in' } });
    const chips = within(row(1)).getByRole('combobox', { name: 'Value' });
    fireEvent.change(chips, { target: { value: 'GET' } });
    fireEvent.keyDown(chips, { key: 'Enter' });
    expect(within(row(1)).getByRole('button', { name: 'Remove GET' })).toBeTruthy();
  });

  it('keeps an unfinished row marked but quiet until a save is attempted', () => {
    const empty = { ...newGroup(), children: [{ ...firstLeaf(sampleRoot()), subject: '' }] };
    render(<Harness initial={empty} />);

    expect(within(row(1)).getByText('Incomplete')).toBeTruthy();
    expect(screen.queryByText('Choose a subject.')).toBeNull();
  });

  it('then shows the message tied to the field it belongs to', () => {
    const empty = { ...newGroup(), children: [{ ...firstLeaf(sampleRoot()), subject: '' }] };
    render(<Harness initial={empty} showIssues />);

    const message = screen.getByText('Choose a subject.');
    const subject = within(row(1)).getByLabelText('Subject');
    expect(subject.getAttribute('aria-describedby')).toBe(message.id);
    expect(subject.getAttribute('aria-invalid')).toBe('true');
  });

  it('shows a server message at once, without waiting for a save attempt', () => {
    const root = sampleRoot();
    const first = root.children[0];
    const serverIssues: IssueMap = new Map([
      [first?.id ?? '', [{ part: 'arg', code: 'regex_invalid', message: 'unclosed group' }]],
    ]);
    render(<Harness initial={root} serverIssues={serverIssues} />);

    expect(within(row(1)).getByText('unclosed group')).toBeTruthy();
    expect(within(row(1)).getByLabelText('Value').getAttribute('aria-invalid')).toBe('true');
  });

  it('stops adding at the leaf limit and says why', () => {
    render(<Harness initial={sampleRoot()} limits={{ maxDepth: 4, maxLeaves: 4 }} />);

    for (const button of screen.getAllByRole('button', { name: 'Add condition' })) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
    }
    expect(screen.getAllByText('Limit of 4 conditions reached.').length).toBeGreaterThan(0);
    expect(screen.getByText('4 of 4 conditions')).toBeTruthy();
  });

  it('refuses a group or a negation that would nest deeper than the limit', () => {
    const nested = { ...newGroup(), children: [newGroup()] };
    render(<Harness initial={nested} limits={{ maxDepth: 2, maxLeaves: 32 }} />);

    const [nestedAdd, rootAdd] = screen.getAllByRole('button', { name: 'Add group' }) as HTMLButtonElement[];
    expect(rootAdd?.disabled).toBe(false);
    expect(nestedAdd?.disabled).toBe(true);
    expect((screen.getByRole('checkbox', { name: 'Negate group 1' }) as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText('Groups can be nested 2 levels deep.')).toBeTruthy();
  });

  it('switches between all and any with a labelled select', () => {
    const seen = vi.fn();
    render(<Harness initial={sampleRoot()} onRoot={seen} />);

    fireEvent.change(screen.getAllByLabelText('Match')[0] as HTMLSelectElement, { target: { value: 'any' } });

    expect(seen).toHaveBeenCalledOnce();
    expect(seen.mock.calls[0]?.[0]).toMatchObject({ mode: 'any' });
  });
});
