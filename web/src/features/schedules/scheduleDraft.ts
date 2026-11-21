import type { FieldError, Schedule, ScheduleInput } from '../../api/types';
import { createKeyValueRow, type KeyValueRow } from '../../components/KeyValueEditor/rows';
import {
  cadenceToExpression,
  cronToCadence,
  DEFAULT_CADENCE,
  DEFAULT_TIMEZONE,
  intervalSeconds,
  isValidEvery,
  isValidTime,
  type CadenceValue,
} from './cadence';
import { isValidTimezone, parseCron } from './cron';

export interface ScheduleDraft {
  name: string;
  cadence: CadenceValue;
  timezone: string;
  method: string;
  url: string;
  headers: KeyValueRow[];
  body: string;
}

export interface DraftIssue {
  code: string;
  message: string;
}

export type IssueMap = ReadonlyMap<string, DraftIssue>;

export const NAME_MAX_LENGTH = 100;
export const NO_ISSUES: IssueMap = new Map();

export function blankScheduleDraft(): ScheduleDraft {
  return {
    name: '',
    cadence: DEFAULT_CADENCE,
    timezone: DEFAULT_TIMEZONE,
    method: 'GET',
    url: '',
    headers: [],
    body: '',
  };
}

export function scheduleDraftOf(schedule: Schedule): ScheduleDraft {
  return {
    name: schedule.name,
    cadence: cronToCadence(schedule.cron),
    timezone: schedule.timezone,
    method: schedule.request.method,
    url: schedule.request.url,
    headers: schedule.request.headers.map((header) => createKeyValueRow(header.name, header.value)),
    body: schedule.request.body ?? '',
  };
}

export function serializeScheduleDraft(draft: ScheduleDraft): string {
  return JSON.stringify({ ...draft, headers: draft.headers.map(({ name, value }) => ({ name, value })) });
}

export function scheduleInputOf(draft: ScheduleDraft): ScheduleInput {
  return {
    name: draft.name.trim(),
    cron: cadenceToExpression(draft.cadence),
    timezone: draft.timezone,
    request: {
      method: draft.method.trim(),
      url: draft.url.trim(),
      headers: draft.headers.map((row) => ({ name: row.name.trim(), value: row.value.trim() })),
      body: draft.body === '' ? null : draft.body,
    },
  };
}

function issue(code: string): DraftIssue {
  return { code, message: '' };
}

export function headerPath(index: number, part: 'name' | 'value'): string {
  return `request.headers.${index}.${part}`;
}

function cadenceIssue(draft: ScheduleDraft, minimumIntervalSeconds: number): [string, DraftIssue] | null {
  const { cadence } = draft;
  if (cadence.kind === 'interval') {
    if (!isValidEvery(cadence.every, cadence.unit)) {
      return ['every', issue('every_invalid')];
    }
    return intervalSeconds(cadence.every, cadence.unit) < minimumIntervalSeconds
      ? ['every', issue('interval_too_short')]
      : null;
  }
  if (cadence.kind === 'daily') {
    return isValidTime(cadence.time) ? null : ['time', issue('time_invalid')];
  }
  return parseCron(cadence.expression) === null ? ['cron', issue('cron_invalid')] : null;
}

export function validateScheduleDraft(draft: ScheduleDraft, minimumIntervalSeconds: number): IssueMap {
  const issues = new Map<string, DraftIssue>();
  const name = draft.name.trim();
  if (name === '') {
    issues.set('name', issue('required'));
  } else if (name.length > NAME_MAX_LENGTH) {
    issues.set('name', issue('too_long'));
  }
  if (draft.method.trim() === '') {
    issues.set('request.method', issue('required'));
  }
  if (draft.url.trim() === '') {
    issues.set('request.url', issue('required'));
  }
  const cadence = cadenceIssue(draft, minimumIntervalSeconds);
  if (cadence !== null) {
    issues.set(cadence[0], cadence[1]);
  }
  if (!isValidTimezone(draft.timezone)) {
    issues.set('timezone', issue('timezone_invalid'));
  }
  draft.headers.forEach((row, index) => {
    if (row.name.trim() === '' && row.value.trim() !== '') {
      issues.set(headerPath(index, 'name'), issue('required'));
    }
  });
  return issues;
}

export function issuesFromServer(fields: readonly FieldError[]): IssueMap {
  const issues = new Map<string, DraftIssue>();
  for (const field of fields) {
    issues.set(field.field, { code: field.code ?? 'invalid', message: field.message });
  }
  return issues;
}

export function firstIssue(issues: IssueMap, paths: readonly string[]): DraftIssue | null {
  for (const path of paths) {
    const found = issues.get(path);
    if (found !== undefined) {
      return found;
    }
  }
  return null;
}
