import { cleanup, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CaptureSummary } from '../../api/types';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import {
  DNS_NAME,
  dnsCaptureFixture,
  emailCaptureFixture,
  emailWithoutSubjectFixture,
  scheduleCaptureFixture,
} from '../../test/fixtures/kinds';
import { summaryOf } from '../../test/msw/store';
import { createI18nInstance } from '../../i18n/setup';
import { CaptureRow } from './CaptureRow';

function renderRow(summary: CaptureSummary) {
  render(
    <I18nextProvider i18n={createI18nInstance()}>
      <CaptureRow
        optionId="row-1"
        summary={summary}
        variant="one-line"
        selected={false}
        positionInSet={1}
        setSize={1}
        now={Date.parse(summary.received_at) + 60_000}
        style={{}}
        onSelect={vi.fn()}
      />
    </I18nextProvider>,
  );
}

afterEach(() => {
  cleanup();
});

describe('CaptureRow by kind', () => {
  it('keeps the method badge and the path for HTTP captures', () => {
    renderRow(summaryOf(jsonCaptureFixture));

    expect(screen.getByText('POST')).toBeTruthy();
    expect(screen.getByText('/404/orders?x=1')).toBeTruthy();
    expect(screen.queryByText('MAIL')).toBeNull();
  });

  it('shows the mail badge with the subject, or the no-subject text when there is none', () => {
    renderRow(summaryOf(emailCaptureFixture));
    expect(screen.getByText('MAIL')).toBeTruthy();
    expect(screen.getByText('Invoice 2026-11')).toBeTruthy();
    expect(screen.queryByText('SMTP')).toBeNull();
    cleanup();

    renderRow(summaryOf(emailWithoutSubjectFixture));
    expect(screen.getByText('(no subject)')).toBeTruthy();
  });

  it('shows the DNS badge with the query name and the asking address', () => {
    renderRow(summaryOf(dnsCaptureFixture));

    expect(screen.getByText('DNS')).toBeTruthy();
    expect(screen.getByText(`x1.${DNS_NAME}`)).toBeTruthy();
    expect(screen.getByText('198.51.100.53')).toBeTruthy();
  });

  it('marks scheduled and imported captures in the second line', () => {
    renderRow({ ...summaryOf(scheduleCaptureFixture), imported: true });

    expect(screen.getByText('POST')).toBeTruthy();
    expect(screen.getByText('Scheduled')).toBeTruthy();
    expect(screen.getByText('Imported')).toBeTruthy();
  });
});
