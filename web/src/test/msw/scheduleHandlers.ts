import { http, type HttpHandler } from 'msw';
import type { Schedule, ScheduleInput, ScheduleRun } from '../../api/types';
import { emptyResponse, jsonResponse, readJsonBody } from './responses';

export interface ScheduleApiState {
  schedules: Schedule[];
  runs: ScheduleRun[];
}

const SCHEDULES = '/api/v1/endpoints/:endpointId/schedules';
const SCHEDULE = '/api/v1/endpoints/:endpointId/schedules/:scheduleId';
const SCHEDULE_RUN = '/api/v1/endpoints/:endpointId/schedules/:scheduleId/run';
const SCHEDULE_RUNS = '/api/v1/endpoints/:endpointId/schedules/:scheduleId/runs';
const CREATED_AT = '2026-11-21T10:20:00.000Z';

function applyInput(schedule: Schedule, input: ScheduleInput): Schedule {
  return {
    ...schedule,
    name: input.name ?? schedule.name,
    cron: input.cron ?? schedule.cron,
    timezone: input.timezone ?? schedule.timezone,
    request: input.request ?? schedule.request,
    enabled: input.enabled ?? schedule.enabled,
  };
}

export function scheduleHandlers(state: ScheduleApiState): HttpHandler[] {
  return [
    http.get(SCHEDULES, () => jsonResponse({ items: state.schedules })),
    http.post(SCHEDULES, async ({ request }) => {
      const input = (await readJsonBody<ScheduleInput>(request)) ?? {};
      const created: Schedule = {
        id: `0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4b${String(state.schedules.length).padStart(2, '0')}`,
        name: input.name ?? '',
        cron: input.cron ?? '',
        timezone: input.timezone ?? 'UTC',
        request: input.request ?? { method: 'GET', url: '', headers: [], body: null },
        enabled: true,
        paused_reason: null,
        next_run_at: null,
        last_run_at: null,
        consecutive_failures: 0,
        created_at: CREATED_AT,
      };
      state.schedules.push(created);
      return jsonResponse(created, 201);
    }),
    http.patch(SCHEDULE, async ({ request, params }) => {
      const scheduleId = String(params.scheduleId);
      const input = (await readJsonBody<ScheduleInput>(request)) ?? {};
      const index = state.schedules.findIndex((item) => item.id === scheduleId);
      const current = state.schedules[index];
      if (index === -1 || current === undefined) {
        return jsonResponse({ error: { code: 'schedule_not_found', message: 'Schedule not found.' } }, 404);
      }
      const updated = applyInput(current, input);
      state.schedules[index] = updated;
      return jsonResponse(updated);
    }),
    http.delete(SCHEDULE, ({ params }) => {
      const scheduleId = String(params.scheduleId);
      state.schedules = state.schedules.filter((item) => item.id !== scheduleId);
      return emptyResponse();
    }),
    http.post(SCHEDULE_RUN, ({ params }) => {
      const scheduleId = String(params.scheduleId);
      return jsonResponse({ schedule_id: scheduleId, queued_at: CREATED_AT }, 202);
    }),
    http.get(SCHEDULE_RUNS, () => jsonResponse({ items: state.runs })),
  ];
}
