import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Capture } from '../../api/types';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { createI18nInstance } from '../../i18n/setup';
import { captureRunsFixture } from '../rules/runlogs/fixtures';
import { CaptureDetail, type CaptureDetailProps } from './CaptureDetail';

afterEach(() => {
  cleanup();
});

function renderDetail(capture: Capture) {
  const queryClient = new QueryClient();
  const props: CaptureDetailProps = {
    state: 'ready',
    capture,
    loadBody: () => Promise.resolve(new Uint8Array(0)),
    bodyMode: 'pretty',
    onBodyModeChange: vi.fn(),
    wrap: false,
    onWrapChange: vi.fn(),
    collapsedSections: [],
    onCollapsedSectionsChange: vi.fn(),
    onResend: vi.fn(),
    onDownloadRaw: vi.fn(),
    onDownloadFormFile: vi.fn(),
    onCopied: vi.fn(),
  };
  render(
    <I18nextProvider i18n={createI18nInstance()}>
      <QueryClientProvider client={queryClient}>
        <CaptureDetail {...props} />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

describe('capture detail run sections', () => {
  it('shows Response sent and Rules for a capture with rule runs', () => {
    renderDetail({ ...jsonCaptureFixture, rule_runs: captureRunsFixture });

    expect(screen.getByRole('heading', { name: 'Response sent' })).not.toBeNull();
    expect(screen.getByRole('heading', { name: 'Rules' })).not.toBeNull();
  });

  it('leaves both sections out for a capture without rule runs', () => {
    renderDetail({ ...jsonCaptureFixture, rule_runs: [] });

    expect(screen.queryByRole('heading', { name: 'Rules' })).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Response sent' })).toBeNull();
  });
});
