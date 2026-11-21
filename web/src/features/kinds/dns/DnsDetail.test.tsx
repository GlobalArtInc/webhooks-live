import { cleanup, render, screen, within } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it } from 'vitest';
import type { Capture } from '../../../api/types';
import { dnsCaptureFixture, DNS_NAME } from '../../../test/fixtures/kinds';
import { createI18nInstance } from '../../../i18n/setup';
import { DnsDetail } from './DnsDetail';

const HOSTILE_NAME = '<img src=x onerror="alert(1)">.dns.hooks.example.test';

function renderDetail(capture: Capture, dnsName: string | null = DNS_NAME) {
  render(
    <I18nextProvider i18n={createI18nInstance()}>
      <DnsDetail capture={capture} dnsName={dnsName} />
    </I18nextProvider>,
  );
}

afterEach(() => {
  cleanup();
});

describe('DnsDetail', () => {
  it('shows the question, the label, the asking address, the transport and the flags', () => {
    renderDetail(dnsCaptureFixture);

    const summary = screen.getByRole('region', { name: 'Summary' });
    expect(within(summary).getByText(`x1.${DNS_NAME}`)).toBeTruthy();
    expect(within(summary).getByText('A (1)')).toBeTruthy();
    expect(within(summary).getByText('IN')).toBeTruthy();
    expect(within(summary).getByText('x1')).toBeTruthy();
    expect(within(summary).getByText('198.51.100.53')).toBeTruthy();
    expect(within(summary).getByText('UDP')).toBeTruthy();

    const query = screen.getByRole('region', { name: 'Query' });
    expect(within(query).getByText('RD, recursion desired; DO, DNSSEC OK')).toBeTruthy();
    expect(within(query).getByText('4096')).toBeTruthy();
    expect(within(query).getByText('35614')).toBeTruthy();
  });

  it('renders a hostile query name as text and says so when the meta cannot be read', () => {
    const hostile: Capture = {
      ...dnsCaptureFixture,
      meta: { ...dnsCaptureFixture.meta, name: HOSTILE_NAME },
    };
    const { container } = render(
      <I18nextProvider i18n={createI18nInstance()}>
        <DnsDetail capture={hostile} dnsName={null} />
      </I18nextProvider>,
    );

    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByText(HOSTILE_NAME)).toBeTruthy();
    cleanup();

    renderDetail({ ...dnsCaptureFixture, meta: null });
    expect(screen.getByRole('status').textContent).toBe('The DNS details of this capture could not be read.');
  });
});
