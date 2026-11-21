import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../../../api/client';
import { I18nProvider } from '../../../i18n/provider';
import { PREFS_KEY } from './constants';
import { fixtureTesterTransport } from './fixtureTransport';
import { TESTER_DRAFT, TESTER_ENDPOINT_ID, TESTER_ENDPOINT_URL, TESTER_NEW_CAPTURE_ID } from './fixtures';
import { parseRuleRun } from './parse';
import { RuleTester } from './RuleTester';
import type { RuleTesterProps } from './props';
import type { TesterDraft, TesterTransport } from './types';

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

let client = new QueryClient();

beforeEach(() => {
  window.localStorage.setItem(PREFS_KEY, JSON.stringify({ autoRun: false }));
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
});

function transportWith(overrides: Partial<TesterTransport> = {}): TesterTransport {
  return {
    ...fixtureTesterTransport,
    dryRun: vi.fn(fixtureTesterTransport.dryRun),
    sendReal: vi.fn(fixtureTesterTransport.sendReal),
    ...overrides,
  };
}

function view(props: Partial<RuleTesterProps>, transport: TesterTransport) {
  return (
    <QueryClientProvider client={client}>
      <I18nProvider bundles={{}}>
        <RuleTester
          endpointId={TESTER_ENDPOINT_ID}
          endpointUrl={TESTER_ENDPOINT_URL}
          draft={TESTER_DRAFT}
          transport={transport}
          {...props}
        />
      </I18nProvider>
    </QueryClientProvider>
  );
}

function renderTester(props: Partial<RuleTesterProps> = {}, transport: TesterTransport = transportWith()) {
  return { transport, ...render(view(props, transport)) };
}

async function runManually(): Promise<void> {
  const button = await screen.findByRole('button', { name: 'Run' });
  await waitFor(() => {
    expect((button as HTMLButtonElement).disabled).toBe(false);
  });
  fireEvent.click(button);
  await screen.findByRole('list', { name: 'Trace of the test run' });
}

function entry(id: string): HTMLElement {
  return screen.getByTestId(`trace-${id}`);
}

describe('RuleTester', () => {
  it('runs the draft on demand and selects the conditions entry first', async () => {
    renderTester();

    await runManually();

    expect(entry('rule-0').getAttribute('aria-current')).toBe('step');
    expect(within(entry('rule-0')).getByText('matched')).toBeTruthy();
    expect(within(entry('step-0-3')).getByText('simulated')).toBeTruthy();
    expect(within(entry('step-0-2')).getByText('runs after the response')).toBeTruthy();
    expect(screen.getByText(/Sender gets 200/)).toBeTruthy();
  });

  it('steps through the trace with the buttons and the bracket keys', async () => {
    const onSelectionChange = vi.fn();
    renderTester({ onSelectionChange });
    await runManually();
    const region = screen.getByRole('region', { name: 'Test' });

    fireEvent.click(screen.getByRole('button', { name: 'Step' }));
    expect(entry('step-0-0').getAttribute('aria-current')).toBe('step');
    expect(within(entry('step-0-1')).getByText('not reached yet')).toBeTruthy();
    expect(onSelectionChange).toHaveBeenLastCalledWith({
      itemId: 'step-0-0',
      ruleId: 'draft-0',
      stepIndex: 0,
    });

    fireEvent.keyDown(region, { key: ']' });
    expect(entry('step-0-1').getAttribute('aria-current')).toBe('step');
    fireEvent.keyDown(region, { key: '[' });
    expect(entry('step-0-0').getAttribute('aria-current')).toBe('step');

    fireEvent.click(screen.getByRole('button', { name: 'Reset' }));
    expect(entry('rule-0').getAttribute('aria-current')).toBe('step');
    expect(within(entry('step-0-0')).getByText('not reached yet')).toBeTruthy();
  });

  it('ignores the bracket keys while typing in a field', async () => {
    renderTester();
    await runManually();

    fireEvent.keyDown(screen.getByRole('searchbox', { name: 'Filter variables' }), { key: ']' });

    expect(entry('rule-0').getAttribute('aria-current')).toBe('step');
  });

  it('marks the result out of date after an edit and clears the mark on the next run', async () => {
    const transport = transportWith();
    const { rerender } = renderTester({}, transport);
    await runManually();
    expect(screen.queryByText('Out of date')).toBeNull();
    const edited: TesterDraft = { ...TESTER_DRAFT, input: { ...TESTER_DRAFT.input, name: 'Edited' } };

    rerender(view({ draft: edited }, transport));

    expect(await screen.findByText('Out of date')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Run' }));
    await waitFor(() => {
      expect(screen.queryByText('Out of date')).toBeNull();
    });
    expect(transport.dryRun).toHaveBeenCalledTimes(2);
  });

  it('re-runs by itself after the debounce when auto-run is on, and never while the draft is blocked', async () => {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify({ autoRun: true }));
    const transport = transportWith();
    const { rerender } = renderTester({}, transport);
    await screen.findByRole('list', { name: 'Trace of the test run' });
    expect(transport.dryRun).toHaveBeenCalledTimes(1);

    rerender(view({ draft: { ...TESTER_DRAFT, input: { ...TESTER_DRAFT.input, name: 'One' } } }, transport));
    await waitFor(() => {
      expect(transport.dryRun).toHaveBeenCalledTimes(2);
    });

    rerender(
      view({ blocked: true, draft: { ...TESTER_DRAFT, input: { ...TESTER_DRAFT.input, name: 'Two' } } }, transport),
    );
    await new Promise((resolve) => setTimeout(resolve, 1000));
    expect(transport.dryRun).toHaveBeenCalledTimes(2);
    expect((screen.getByRole('button', { name: 'Run' }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/Fix the errors in the draft/)).toBeTruthy();
  });

  it('does not run for a viewer', async () => {
    const transport = transportWith();
    renderTester({ canEdit: false }, transport);

    expect(await screen.findByText('Only editors can run tests.')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Run' }) as HTMLButtonElement).disabled).toBe(true);
    expect(transport.dryRun).not.toHaveBeenCalled();
  });

  it('shows every rule of the chain, where a stop cut it, and which rule answered', async () => {
    renderTester({ defaultScope: 'all' });

    await runManually();

    const rules = await screen.findByRole('list', { name: 'Rules in run order' });
    await waitFor(() => {
      expect(within(rules).getByText('Stop here: later rules are skipped')).toBeTruthy();
    });
    expect(within(rules).getByText('not run')).toBeTruthy();
    expect(screen.getByText(/Sender gets 401/)).toBeTruthy();
    expect(screen.getByText(/\(rule 2\)/)).toBeTruthy();
  });

  it('lists the side effects before a real run, sends nothing on cancel and links the new capture', async () => {
    const transport = transportWith();
    renderTester({ dirty: true }, transport);
    const trigger = await screen.findByRole('button', { name: 'Run for real...' });
    await waitFor(() => {
      expect((trigger as HTMLButtonElement).disabled).toBe(false);
    });

    fireEvent.click(trigger);
    const dialog = await screen.findByRole('dialog', { name: 'Run for real?' });
    expect(within(dialog).getByText(/Http, Increment/)).toBeTruthy();
    expect(within(dialog).getByText(/saved version runs/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(transport.sendReal).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Run for real...' }));
    fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Run for real' }));

    const link = await screen.findByRole('link', { name: 'Open the new capture (new tab)' });
    expect(link.getAttribute('href')).toBe(`/e/${TESTER_ENDPOINT_ID}/c/${TESTER_NEW_CAPTURE_ID}`);
    expect(link.getAttribute('target')).toBe('_blank');
    expect(link.getAttribute('rel')).toContain('noopener');
    expect(transport.sendReal).toHaveBeenCalledTimes(1);
  });

  it('shows server field errors of a refused run', async () => {
    const transport = transportWith({
      dryRun: () =>
        Promise.reject(
          new ApiError({
            status: 422,
            code: 'validation_failed',
            message: 'The rules are invalid.',
            details: [{ field: 'rules.0.steps.0.kind', message: 'is not a known step type' }],
            retryAfter: null,
          }),
        ),
    });
    renderTester({}, transport);

    await runFailing();

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('The rules are invalid.');
    expect(alert.textContent).toContain('rules.0.steps.0.kind: is not a known step type');
  });

  it('renders every piece of server output as text', async () => {
    const hostile = '<img src=x onerror=alert(1)>';
    const transport = transportWith({
      dryRun: () =>
        Promise.resolve(
          parseRuleRun({
            id: 'run',
            capture_id: 'capture',
            state: 'failed',
            rules_version: 1,
            response_status: null,
            error: `script: ${hostile}`,
            entries: [
              { rule_id: 'r1', rule_name: hostile, matched: true },
              {
                rule_id: 'r1',
                step: 0,
                name: null,
                kind: 'script',
                outcome: 'failed',
                duration_ms: 1,
                detail: hostile,
                output: { log: `<b>bold</b>\n${hostile}` },
              },
            ],
            created_at: '2026-11-21T10:00:00.000Z',
            finished_at: null,
          }),
        ),
    });
    const { container } = renderTester({}, transport);

    await runManually();
    fireEvent.click(screen.getByRole('button', { name: 'Step' }));

    expect(screen.getByLabelText('Script log lines').textContent).toContain('<b>bold</b>');
    expect(container.querySelector('img')).toBeNull();
    expect(document.body.querySelector('b')).toBeNull();
    expect(screen.getAllByText(new RegExp(hostile.replace(/[()]/g, '\\$&'))).length).toBeGreaterThan(0);
  });
});

async function runFailing(): Promise<void> {
  const button = await screen.findByRole('button', { name: 'Run' });
  await waitFor(() => {
    expect((button as HTMLButtonElement).disabled).toBe(false);
  });
  fireEvent.click(button);
}
