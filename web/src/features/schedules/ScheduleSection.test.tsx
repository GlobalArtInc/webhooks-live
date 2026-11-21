import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import type { Instance, Schedule, ScheduleInput } from '../../api/types';
import { ENDPOINT_ID, endpointFixture } from '../../test/fixtures/endpoint';
import { instanceFixture } from '../../test/fixtures/instance';
import {
  pausedScheduleFixture,
  SCHEDULE_ID,
  scheduleFixture,
  scheduleRunFixtures,
} from '../../test/fixtures/schedules';
import { renderAppAt } from '../endpoints/renderApp';
import { scheduleHandlers, type ScheduleApiState } from '../../test/msw/scheduleHandlers';
import { errorResponse, jsonResponse, readJsonBody } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';

registerMockServer();

afterEach(() => {
  cleanup();
});

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/schedules`;
const SCHEDULES_PATH = `/api/v1/endpoints/${ENDPOINT_ID}/schedules`;
const SCHEDULE_PATH = `${SCHEDULES_PATH}/${SCHEDULE_ID}`;
const INSTANCE_PATH = '/api/v1/instance';
const PREVIEW_COUNT = 5;

function enableSchedules(): void {
  const instance: Instance = { ...instanceFixture, features: { ...instanceFixture.features, schedules: true } };
  mockServer.use(http.get(INSTANCE_PATH, () => jsonResponse(instance)));
}

function ownSchedules(schedules: Schedule[]): ScheduleApiState {
  return { schedules: [...schedules], runs: [...scheduleRunFixtures] };
}

function valueOf(label: string): string {
  return (screen.getByLabelText(label) as HTMLInputElement).value;
}

describe('schedule section', () => {
  it('explains that the feature is off when the instance does not allow schedules', async () => {
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    renderAppAt(SECTION_PATH);

    expect(await screen.findByText('Scheduled requests are not available on this instance.')).toBeTruthy();
  });

  it('lists schedules with their state and opens the request of a paused one', async () => {
    enableSchedules();
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    mockServer.use(...scheduleHandlers(ownSchedules([scheduleFixture, pausedScheduleFixture])));
    renderAppAt(SECTION_PATH);

    const list = await screen.findByRole('list', { name: 'Schedules' });
    expect(within(list).getByText('Paused')).toBeTruthy();
    fireEvent.click(within(list).getByRole('button', { name: /Nightly report/ }));

    expect(await screen.findByRole('heading', { level: 3, name: 'Nightly report' })).toBeTruthy();
    expect(valueOf('URL')).toBe('https://reports.example.test/nightly');
    expect(valueOf('Method')).toBe('POST');
    expect(screen.getByRole('switch', { name: 'Schedule active' }).getAttribute('aria-checked')).toBe('false');
  });

  it('previews the next five runs of the cadence as text', async () => {
    enableSchedules();
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    mockServer.use(...scheduleHandlers(ownSchedules([])));
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'New schedule' }));

    const preview = await screen.findByRole('region', { name: 'Next runs' });
    expect(within(preview).getAllByRole('listitem')).toHaveLength(PREVIEW_COUNT);
  });

  it('shows the server field error on the cadence after a rejected save', async () => {
    enableSchedules();
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    mockServer.use(...scheduleHandlers(ownSchedules([])));
    mockServer.use(
      http.post(SCHEDULES_PATH, () =>
        errorResponse(422, 'validation_failed', 'The schedule is invalid.', [
          { field: 'cron', code: 'cron_invalid', message: 'Unexpected token' },
        ]),
      ),
    );
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'New schedule' }));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Weekly ping' } });
    fireEvent.change(screen.getByLabelText('URL'), { target: { value: 'https://staging.example.test/weekly' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByText('Not a valid five-field cron expression.')).toBeTruthy();
    expect(screen.getByRole('textbox', { name: 'Name' }).getAttribute('aria-invalid')).toBeNull();
  });

  it('pauses an active schedule at once with a single state change', async () => {
    enableSchedules();
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    const bodies: ScheduleInput[] = [];
    mockServer.use(...scheduleHandlers(ownSchedules([scheduleFixture])));
    mockServer.use(
      http.patch(SCHEDULE_PATH, async ({ request }) => {
        bodies.push((await readJsonBody<ScheduleInput>(request)) ?? {});
        return jsonResponse({ ...scheduleFixture, enabled: false });
      }),
    );
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: /Ping staging/ }));
    fireEvent.click(await screen.findByRole('switch', { name: 'Schedule active' }));

    await waitFor(() => expect(bodies).toEqual([{ enabled: false }]));
  });

  it('shows the run history with its capture link and narrows it to failed runs', async () => {
    enableSchedules();
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    mockServer.use(...scheduleHandlers(ownSchedules([scheduleFixture])));
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: /Ping staging/ }));
    const table = await screen.findByRole('table', { name: 'Run history' });
    expect(within(table).getAllByRole('row')).toHaveLength(scheduleRunFixtures.length + 1);
    expect(within(table).getByRole('link', { name: 'Open' })).toBeTruthy();

    fireEvent.click(screen.getByRole('checkbox', { name: 'Failed only' }));
    const failedOnly = screen.getByRole('table', { name: 'Run history' });
    expect(within(failedOnly).getAllByRole('row')).toHaveLength(3);
    expect(within(failedOnly).getByText('connection refused')).toBeTruthy();
  });
});
