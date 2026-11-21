import { http, type HttpHandler } from 'msw';
import type { FieldError } from '../../api/types';
import { simulateDryRun } from '../../features/rules/tester/fixtureEngine';
import { errorResponse, jsonResponse, readJsonBody } from './responses';
import { mockStore } from './store';

interface EndpointPath {
  endpointId: string;
}

const DRY_RUN = '/api/v1/endpoints/:endpointId/rules/dry-run';
const FIELDS: readonly string[] = ['capture_id', 'rules', 'execute_http'];
const MAX_RULES = 100;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function violation(field: string, code: string, message: string): FieldError[] {
  return [{ field, code, message }];
}

function validate(body: Record<string, unknown>): FieldError[] {
  const unknown = Object.keys(body).find((key) => !FIELDS.includes(key));
  if (unknown !== undefined) {
    return violation(unknown, 'not_allowed', 'is not a known field');
  }
  if (body.capture_id === undefined || body.capture_id === null) {
    return violation('capture_id', 'required', 'is required');
  }
  if (typeof body.capture_id !== 'string') {
    return violation('capture_id', 'invalid_format', 'must be a string');
  }
  if (body.execute_http !== undefined && body.execute_http !== null && typeof body.execute_http !== 'boolean') {
    return violation('execute_http', 'invalid_format', 'must be a boolean');
  }
  if (body.rules !== undefined && body.rules !== null) {
    if (!Array.isArray(body.rules)) {
      return violation('rules', 'invalid_format', 'must be an array');
    }
    if (body.rules.length > MAX_RULES) {
      return violation('rules', 'too_many', `must have at most ${MAX_RULES} rules`);
    }
  }
  return [];
}

export const testerHandlers: HttpHandler[] = [
  http.post<EndpointPath>(DRY_RUN, async ({ params, request }) => {
    const body = await readJsonBody<unknown>(request);
    if (!isRecord(body)) {
      return errorResponse(400, 'invalid_json', 'Body must be a JSON object.');
    }
    const fields = validate(body);
    if (fields.length > 0) {
      return errorResponse(422, 'validation_failed', 'The request is invalid.', fields);
    }
    const captures = mockStore.captures.get(params.endpointId) ?? [];
    const capture = captures.find((candidate) => candidate.id === body.capture_id);
    if (capture === undefined) {
      return errorResponse(404, 'capture_not_found', 'Capture not found.');
    }
    const rules: unknown[] = Array.isArray(body.rules) ? body.rules : [];
    const drafts = rules.filter(isRecord);
    if (
      drafts.some(
        (rule) =>
          rule.kind === 'teleport' ||
          (Array.isArray(rule.steps) && rule.steps.some((step) => isRecord(step) && step.kind === 'teleport')),
      )
    ) {
      return errorResponse(
        422,
        'validation_failed',
        'The rules are invalid.',
        violation('rules.0.steps.0.kind', 'invalid_format', 'is not a known step type'),
      );
    }
    return jsonResponse(simulateDryRun(drafts, capture, { extended: false }));
  }),
];
