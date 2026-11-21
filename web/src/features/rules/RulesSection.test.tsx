import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import { renderAppAt } from '../endpoints/renderApp';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { RULE_IDS, rulesFixture } from '../../test/fixtures/rules';
import { resetRulesStore, seedRules } from '../../test/msw/rules';
import { ruleEditorSupportHandlers } from '../../test/msw/ruleEditor';
import { errorResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import type { Rule } from '../../api/rulesTypes';

registerMockServer();

afterEach(() => {
  cleanup();
  resetRulesStore();
});

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/rules`;
const ORDER_URL = `/api/v1/endpoints/${ENDPOINT_ID}/rules/order`;
const LIST_NAME = 'Rules in run order';

function seed(rules: Rule[] = rulesFixture()): void {
  seedEndpoint(endpointFixture, []);
  seedRules(ENDPOINT_ID, rules);
}

function cardLabels(): string[] {
  const list = screen.getByRole('list', { name: LIST_NAME });
  return within(list)
    .getAllByRole('region')
    .map((region) => region.getAttribute('aria-label') ?? '');
}

function switchFor(name: string): HTMLElement {
  return screen.getByRole('switch', { name: `Enabled: ${name}` });
}

describe('rules section', () => {
  it('lists the rules in run order with their switch state', async () => {
    seed();
    renderAppAt(SECTION_PATH);

    expect(await screen.findByRole('list', { name: LIST_NAME })).toBeTruthy();
    expect(cardLabels()).toEqual([
      'Rule 1 of 3: Mark paid orders',
      'Rule 2 of 3: Reject bad signature',
      'Rule 3 of 3: Mail to chat',
    ]);
    expect(switchFor('Mail to chat').getAttribute('aria-checked')).toBe('false');
    expect(switchFor('Mark paid orders').getAttribute('aria-checked')).toBe('true');
  });

  it('shows the empty state with the creation links when there are no rules', async () => {
    seed([]);
    renderAppAt(SECTION_PATH);

    expect(await screen.findByText('No rules yet')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'New rule' }).getAttribute('href')).toContain(
      '/settings/rules/new?from=blank',
    );
    expect(screen.getByRole('link', { name: 'New script rule' }).getAttribute('href')).toContain('from=script');
  });

  it('links duplicate to a draft that starts from the rule', async () => {
    seed();
    renderAppAt(SECTION_PATH);

    const link = await screen.findByRole('link', { name: 'Duplicate: Mark paid orders' });
    expect(link.getAttribute('href')).toContain(`/settings/rules/new?from=${RULE_IDS.markPaid}`);
  });

  it('moves a rule with Alt+Shift+Up, announces the new position, and keeps the saved order', async () => {
    seed();
    renderAppAt(SECTION_PATH);

    const link = await screen.findByRole('link', { name: 'Reject bad signature' });
    fireEvent.keyDown(link, { key: 'ArrowUp', altKey: true, shiftKey: true });

    expect(await screen.findByText('Reject bad signature moved to position 1 of 3.')).toBeTruthy();
    await waitFor(() => expect(cardLabels()[0]).toBe('Rule 1 of 3: Reject bad signature'));
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole('link', { name: 'Reject bad signature' })),
    );
  });

  it('puts the order back and shows an alert when the order cannot be saved', async () => {
    seed();
    mockServer.use(http.put(ORDER_URL, () => errorResponse(500, 'internal_error', 'Unexpected failure.')));
    renderAppAt(SECTION_PATH);

    const link = await screen.findByRole('link', { name: 'Reject bad signature' });
    fireEvent.keyDown(link, { key: 'ArrowUp', altKey: true, shiftKey: true });

    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(screen.getByText('The order could not be saved. The previous order is back.')).toBeTruthy();
    await waitFor(() => expect(cardLabels()[0]).toBe('Rule 1 of 3: Mark paid orders'));
  });

  it('switches a rule on at once and offers Undo', async () => {
    seed();
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('switch', { name: 'Enabled: Mail to chat' }));

    expect(await screen.findByText('Mail to chat is on.')).toBeTruthy();
    await waitFor(() => expect(switchFor('Mail to chat').getAttribute('aria-checked')).toBe('true'));
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    await waitFor(() => expect(switchFor('Mail to chat').getAttribute('aria-checked')).toBe('false'));
  });

  it('deletes a rule only after the confirmation', async () => {
    seed();
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Delete: Mail to chat' }));
    const dialog = await screen.findByRole('dialog', { name: 'Delete rule?' });
    expect(within(dialog).getByText('Mail to chat and its steps will be deleted. This cannot be undone.')).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete rule' }));

    await waitFor(() =>
      expect(cardLabels()).toEqual(['Rule 1 of 2: Mark paid orders', 'Rule 2 of 2: Reject bad signature']),
    );
  });

  it('opens a blank draft in the editor route', async () => {
    seed();
    mockServer.use(...ruleEditorSupportHandlers);
    const { router } = renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('link', { name: 'New rule' }));

    expect(await screen.findByRole('heading', { name: 'New rule' })).toBeTruthy();
    expect(screen.getByRole('textbox', { name: 'Rule name' })).toBeTruthy();
    expect(router.state.location.href).toContain('/settings/rules/new?from=blank');
  });

  it('opens the tester in all rules mode from the Test all rules button and closes it again', async () => {
    seed();
    mockServer.use(...ruleEditorSupportHandlers);
    const { router } = renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Test all rules' }));

    expect(await screen.findByRole('region', { name: 'Test' })).toBeTruthy();
    expect(router.state.location.href).toContain('test=all');
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => expect(screen.queryByRole('region', { name: 'Test' })).toBeNull());
    expect(router.state.location.href).not.toContain('test=all');
  });
});
