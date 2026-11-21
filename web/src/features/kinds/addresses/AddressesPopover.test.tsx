import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { endpointFixture } from '../../../test/fixtures/endpoint';
import { DNS_NAME, MAIL_ADDRESS, mailEndpointFixture } from '../../../test/fixtures/kinds';
import { createI18nInstance } from '../../../i18n/setup';
import { EmptySamples } from '../samples/EmptySamples';
import { AddressesPopover } from './AddressesPopover';

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

describe('AddressesPopover and EmptySamples', () => {
  it('lists the HTTP, email and DNS addresses of the endpoint with copy buttons', async () => {
    render(
      <I18nextProvider i18n={createI18nInstance()}>
        <AddressesPopover endpoint={mailEndpointFixture} curl="curl --url https://hooks.example.test/x" />
      </I18nextProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Addresses (3)' }));
    const dialog = await screen.findByRole('dialog', { name: 'Addresses' });

    expect(within(dialog).getByText(MAIL_ADDRESS)).toBeTruthy();
    expect(within(dialog).getByText(DNS_NAME)).toBeTruthy();
    expect(within(dialog).getByText(`any-label.${DNS_NAME} is also caught`)).toBeTruthy();
    expect(within(dialog).getByRole('button', { name: 'Copy email address' })).toBeTruthy();
    expect(within(dialog).getByRole('button', { name: 'Copy dig example' })).toBeTruthy();
  });

  it('shows the email and DNS samples only for the addresses that are enabled', () => {
    const { container, rerender } = render(
      <I18nextProvider i18n={createI18nInstance()}>
        <EmptySamples endpoint={mailEndpointFixture} />
      </I18nextProvider>,
    );

    expect(screen.getByText(/^swaks --to/)).toBeTruthy();
    expect(screen.getByText(/^dig 'test\./)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Copy email example' })).toBeTruthy();

    rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <EmptySamples endpoint={endpointFixture} />
      </I18nextProvider>,
    );
    expect(container.textContent).toBe('');
  });
});
