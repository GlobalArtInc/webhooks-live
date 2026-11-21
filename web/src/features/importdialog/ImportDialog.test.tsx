import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { I18nProvider } from '../../i18n/provider';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { EXPORT_TITLE, jsonExportText, ndjsonExportText } from '../../test/fixtures/exports';
import { importHandlers, type ImportRecord } from '../../test/msw/imports';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { ImportDialog } from './ImportDialog';

registerMockServer();

async function renderDialog(
  endpointId: string | null,
  onOpenEndpoint: (endpointId: string) => void = () => undefined,
): Promise<void> {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <I18nProvider bundles={{}}>
        <ImportDialog endpointId={endpointId} onOpenChange={() => undefined} onOpenEndpoint={onOpenEndpoint} />
      </I18nProvider>
    </QueryClientProvider>,
  );
  await screen.findByRole('dialog');
}

function chooseFile(name: string, text: string, type: string): void {
  const input = document.querySelector<HTMLInputElement>('input[type="file"]');
  if (input === null) {
    throw new Error('The file input is not rendered.');
  }
  fireEvent.change(input, { target: { files: [new File([text], name, { type })] } });
}

function button(name: string): HTMLButtonElement {
  return screen.getByRole('button', { name }) as HTMLButtonElement;
}

async function readyToImport(): Promise<void> {
  await screen.findByText(EXPORT_TITLE);
  await waitFor(() => {
    expect(button('Import').disabled).toBe(false);
  });
}

describe('import dialog', () => {
  afterEach(() => {
    cleanup();
  });

  it('refuses a ZIP export and never uploads it', async () => {
    const requests: ImportRecord[] = [];
    mockServer.use(
      ...importHandlers({
        onRequest: (record) => {
          requests.push(record);
        },
      }),
    );
    await renderDialog(null);

    chooseFile('captures.zip', 'PK', 'application/zip');

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('ZIP files cannot be imported.');
    expect(button('Import').disabled).toBe(true);
    expect(requests).toEqual([]);
  });

  it('shows the header of a JSON export and imports it into a new endpoint', async () => {
    const requests: ImportRecord[] = [];
    const opened: string[] = [];
    mockServer.use(
      ...importHandlers({
        onRequest: (record) => {
          requests.push(record);
        },
      }),
    );
    await renderDialog(null, (endpointId) => {
      opened.push(endpointId);
    });

    chooseFile('example-export.json', jsonExportText(), 'application/json');
    await readyToImport();
    fireEvent.click(button('Import'));

    expect(await screen.findByText('Imported 2 captures.')).toBeTruthy();
    expect(requests).toEqual([
      { path: '/api/v1/endpoints/import', keepTimes: 'true', contentType: 'application/json' },
    ]);
    fireEvent.click(button('Open endpoint'));
    expect(opened).toEqual([ENDPOINT_ID]);
  });

  it('sends an NDJSON export with the keep original times switch turned off', async () => {
    const requests: ImportRecord[] = [];
    mockServer.use(
      ...importHandlers({
        onRequest: (record) => {
          requests.push(record);
        },
      }),
    );
    await renderDialog(null);

    chooseFile('example-export.ndjson', ndjsonExportText(), 'application/x-ndjson');
    await readyToImport();
    fireEvent.click(screen.getByRole('checkbox', { name: /Keep original times/u }));
    fireEvent.click(button('Import'));

    expect(await screen.findByText('Imported 2 captures.')).toBeTruthy();
    expect(requests).toEqual([
      { path: '/api/v1/endpoints/import', keepTimes: 'false', contentType: 'application/x-ndjson' },
    ]);
  });

  it('shows the wait after a busy answer and keeps retry closed until it passes', async () => {
    mockServer.use(
      ...importHandlers({
        into: {
          status: 429,
          body: { error: { code: 'rate_limited', message: 'Wait for the running import.' } },
          headers: { 'Retry-After': '60' },
        },
      }),
    );
    await renderDialog(ENDPOINT_ID);

    chooseFile('example-export.json', jsonExportText(), 'application/json');
    await readyToImport();
    fireEvent.click(button('Import'));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('Another import is running');
    expect(screen.getByText('You can try again in 60 s.')).toBeTruthy();
    expect(button('Try again').disabled).toBe(true);
  });

  it('marks the position of each problem the server reports', async () => {
    mockServer.use(
      ...importHandlers({
        into: {
          status: 422,
          body: {
            error: {
              code: 'validation_failed',
              message: 'The file has problems.',
              fields: [{ field: 'captures.1.status', message: 'must be a status code' }],
            },
          },
        },
      }),
    );
    await renderDialog(ENDPOINT_ID);

    chooseFile('example-export.json', jsonExportText(), 'application/json');
    await readyToImport();
    fireEvent.click(button('Import'));

    expect(await screen.findByText('captures.1.status')).toBeTruthy();
    expect(screen.getByText('The server found problems in the file. Nothing was imported.')).toBeTruthy();
  });
});
