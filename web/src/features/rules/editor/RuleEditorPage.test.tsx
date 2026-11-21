import { cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { renderAppAt } from '../../endpoints/renderApp';
import { endpointFixture, ENDPOINT_ID } from '../../../test/fixtures/endpoint';
import { RULE_IDS, rulesFixture } from '../../../test/fixtures/rules';
import { ruleEditorSupportHandlers } from '../../../test/msw/ruleEditor';
import { resetRulesStore, seedRules } from '../../../test/msw/rules';
import { mockServer, registerMockServer } from '../../../test/msw/server';
import { seedEndpoint } from '../../../test/msw/store';
import { PREFS_KEY } from '../tester/constants';
import type { EndpointRole } from '../../../api/types';

registerMockServer();

const EDITOR_PATH = `/e/${ENDPOINT_ID}/settings/rules/${RULE_IDS.markPaid}`;

function seed(role: EndpointRole | null = 'editor'): void {
  seedEndpoint({ ...endpointFixture, my_role: role }, []);
  seedRules(ENDPOINT_ID, rulesFixture());
}

beforeEach(() => {
  window.localStorage.setItem(PREFS_KEY, JSON.stringify({ autoRun: false }));
  mockServer.use(...ruleEditorSupportHandlers);
});

afterEach(() => {
  cleanup();
  resetRulesStore();
  window.localStorage.clear();
});

describe('rule editor page', () => {
  it('keeps Save disabled until the rule is edited, then saves the edit', async () => {
    seed();
    renderAppAt(EDITOR_PATH);

    const name = await screen.findByRole('textbox', { name: 'Rule name' });
    const save = screen.getByRole('button', { name: 'Save' });
    expect((name as HTMLInputElement).value).toBe('Mark paid orders');
    expect((save as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(name, { target: { value: 'Mark paid orders v2' } });
    await waitFor(() =>
      expect((screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement).disabled).toBe(false),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Rule saved.')).toBeTruthy();
    await waitFor(() =>
      expect((screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement).disabled).toBe(true),
    );
  });

  it('shows the not found state for a rule that does not exist', async () => {
    seed();
    renderAppAt(`/e/${ENDPOINT_ID}/settings/rules/0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4aff`);

    expect(await screen.findByRole('heading', { name: 'Page not found' })).toBeTruthy();
  });

  it('is read-only for a viewer', async () => {
    seed('viewer');
    renderAppAt(EDITOR_PATH);

    const name = await screen.findByRole('textbox', { name: 'Rule name' });
    expect((name as HTMLInputElement).readOnly).toBe(true);
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
    expect(screen.getByText('You can view this rule but not change it.')).toBeTruthy();
  });

  it('opens a blank draft from the list and asks for a step before it can be created', async () => {
    seed();
    renderAppAt(`/e/${ENDPOINT_ID}/settings/rules/new?from=blank`);

    expect(await screen.findByRole('heading', { name: 'New rule' })).toBeTruthy();
    const name = screen.getByRole('textbox', { name: 'Rule name' });
    fireEvent.change(name, { target: { value: 'Brand new' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(screen.getByText('Add at least one step.')).toBeTruthy();
  });
});
