import { cleanup, fireEvent, render, screen } from '@testing-library/react';
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
import { CaptureRow, CaptureRowActions } from './CaptureRow';
import type { CaptureRowVariant } from './CaptureRow';

function renderRow(summary: CaptureSummary, variant: CaptureRowVariant = 'one-line') {
  render(
    <I18nextProvider i18n={createI18nInstance()}>
      <CaptureRow
        optionId="row-1"
        summary={summary}
        variant={variant}
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

describe('CaptureRow in the two-line variant', () => {
  it('keeps the first line for the method and the path, and moves the time and the size to the second', () => {
    renderRow(summaryOf(jsonCaptureFixture), 'two-line');

    const option = screen.getByRole('option');
    const lines = option.children;
    expect(lines).toHaveLength(2);
    expect(lines[0]?.textContent).toContain('/404/orders?x=1');
    expect(lines[0]?.querySelector('time')).toBeNull();
    expect(lines[1]?.querySelector('time')?.textContent).toBe('1 minute ago');
    expect(lines[1]?.textContent).toContain('12 B');
  });
});

describe('CaptureRowActions', () => {
  const handlers = { onDelete: vi.fn(), onCompare: vi.fn() };

  function renderActions(variant: CaptureRowVariant) {
    render(
      <I18nextProvider i18n={createI18nInstance()}>
        <CaptureRowActions captureId="c1" compareLabel="mark" variant={variant} style={{}} {...handlers} />
      </I18nextProvider>,
    );
  }

  it('shows the delete and compare buttons inline in the one-line variant', () => {
    renderActions('one-line');

    fireEvent.click(screen.getByRole('button', { name: 'Delete capture' }));
    fireEvent.click(screen.getByRole('button', { name: 'Mark for compare' }));
    expect(handlers.onDelete).toHaveBeenCalledWith('c1');
    expect(handlers.onCompare).toHaveBeenCalledWith('c1');
    expect(screen.queryByRole('button', { name: 'Capture actions' })).toBeNull();
  });

  it('puts both actions behind one menu button in the two-line variant', async () => {
    renderActions('two-line');

    expect(screen.queryByRole('button', { name: 'Delete capture' })).toBeNull();
    const trigger = screen.getByRole('button', { name: 'Capture actions' });
    fireEvent.keyDown(trigger, { key: 'Enter' });

    fireEvent.click(await screen.findByRole('menuitem', { name: 'Delete capture' }));
    expect(handlers.onDelete).toHaveBeenCalledWith('c1');
  });
});
