import type { Schedule, ScheduleRun } from '../../api/types';

export const SCHEDULE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a51';
export const PAUSED_SCHEDULE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a52';
export const CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a61';

export const scheduleFixture: Schedule = {
  id: SCHEDULE_ID,
  name: 'Ping staging',
  cron: '*/5 * * * *',
  timezone: 'UTC',
  request: {
    method: 'GET',
    url: 'https://staging.example.test/health',
    headers: [{ name: 'Authorization', value: 'Bearer {{ secret.EXAMPLE_TOKEN }}' }],
    body: null,
  },
  enabled: true,
  paused_reason: null,
  next_run_at: '2026-11-21T10:15:00.000Z',
  last_run_at: '2026-11-21T10:10:00.000Z',
  consecutive_failures: 0,
  created_at: '2026-11-20T09:00:00.000Z',
};

export const pausedScheduleFixture: Schedule = {
  id: PAUSED_SCHEDULE_ID,
  name: 'Nightly report',
  cron: '0 2 * * *',
  timezone: 'Europe/Berlin',
  request: {
    method: 'POST',
    url: 'https://reports.example.test/nightly',
    headers: [],
    body: '{"report": "nightly"}',
  },
  enabled: false,
  paused_reason: 'Paused after 10 failures in a row',
  next_run_at: null,
  last_run_at: '2026-11-19T01:00:00.000Z',
  consecutive_failures: 10,
  created_at: '2026-11-18T09:00:00.000Z',
};

export const scheduleRunFixtures: ScheduleRun[] = [
  {
    id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a71',
    started_at: '2026-11-21T10:10:00.000Z',
    status: 200,
    duration_ms: 88,
    error: null,
    capture_id: CAPTURE_ID,
  },
  {
    id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a72',
    started_at: '2026-11-21T10:05:00.000Z',
    status: 503,
    duration_ms: 1200,
    error: null,
    capture_id: null,
  },
  {
    id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a73',
    started_at: '2026-11-21T10:00:00.000Z',
    status: null,
    duration_ms: null,
    error: 'connection refused',
    capture_id: null,
  },
];
