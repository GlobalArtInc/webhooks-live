import { cleanup, fireEvent, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import type { OrganizedEndpoint } from '../folders/model';
import { avatarText } from './CollapsedRail';
import { renderAppAt } from './renderApp';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints, type MyEndpoint } from './storage';

registerMockServer();

const OTHER_ID = 'k7m2q9x4w8c1d5f3h6j0n2p4';

function row(label: string | null, id = ENDPOINT_ID): OrganizedEndpoint {
  return { id, source: 'local', role: null, label, folderKey: null, tags: [], captureCount: null, lastActivity: 0 };
}

function seedTwo(): void {
  const entries: MyEndpoint[] = [
    { id: ENDPOINT_ID, label: 'stripe-dev', added: 1, lastOpened: 2, lastSeenCaptureId: null },
    { id: OTHER_ID, label: null, added: 1, lastOpened: 1, lastSeenCaptureId: null },
  ];
  window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, serializeEndpoints(entries));
}

async function collapsedRail(): Promise<HTMLElement> {
  const nav = await screen.findByRole('navigation', { name: 'My endpoints' });
  await within(nav).findByRole('link', { name: 'stripe-dev' });
  fireEvent.click(within(nav).getByRole('button', { name: 'Collapse sidebar' }));
  await within(nav).findByRole('button', { name: 'Expand sidebar' });
  return nav;
}

describe('avatarText', () => {
  it('takes the first two characters of the title, upper-cased, or of the id when there is no title', () => {
    expect(avatarText(row('stripe-dev'))).toBe('ST');
    expect(avatarText(row('  hooks'))).toBe('HO');
    expect(avatarText(row(null))).toBe('7K');
    expect(avatarText(row('', OTHER_ID))).toBe('K7');
    expect(avatarText(row('😀😀x'))).toBe('😀😀');
  });
});

describe('collapsed rail', () => {
  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it('offers a named control for every item and marks the open endpoint as current', async () => {
    seedEndpoint(endpointFixture);
    seedTwo();
    renderAppAt(`/e/${ENDPOINT_ID}`);

    const nav = await collapsedRail();

    for (const name of ['Expand sidebar', 'New endpoint', 'Folders', 'Labels']) {
      expect(within(nav).getByRole('button', { name })).toBeTruthy();
    }
    expect(within(nav).getByRole('link', { name: 'My endpoints' }).getAttribute('href')).toBe('/endpoints');
    expect(within(nav).getByRole('link', { name: 'Composer' }).getAttribute('href')).toBe('/send');
    expect(within(nav).getByRole('link', { name: 'stripe-dev' }).getAttribute('aria-current')).toBe('page');
    expect(within(nav).getByRole('link', { name: 'k7m2q9...2p4' }).getAttribute('aria-current')).toBeNull();
    expect(nav.querySelectorAll('svg:not([aria-hidden="true"])')).toHaveLength(0);
  });

  it('expands the sidebar from the Folders item', async () => {
    seedTwo();
    renderAppAt('/');
    const nav = await collapsedRail();

    fireEvent.click(within(nav).getByRole('button', { name: 'Folders' }));

    expect(within(nav).getByRole('button', { name: 'Collapse sidebar' }).getAttribute('aria-expanded')).toBe('true');
  });

  it('shows the full label as a tooltip on hover and closes it on Escape', async () => {
    seedTwo();
    renderAppAt('/');
    const nav = await collapsedRail();
    const composer = within(nav).getByRole('link', { name: 'Composer' });

    fireEvent.mouseEnter(composer.parentElement as HTMLElement);

    const tip = within(nav)
      .getAllByText('Composer')
      .find((node) => node.getAttribute('class')?.includes('bubble'));
    expect(tip).toBeTruthy();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(within(nav).getAllByText('Composer')).toHaveLength(1);
  });
});
