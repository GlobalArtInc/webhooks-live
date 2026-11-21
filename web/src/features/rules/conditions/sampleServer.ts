import { ApiError } from '../../../api/client';
import type { FieldError } from '../../../api/types';
import type { ConditionsInput, RuleConditionsSource } from './types';

export interface SampleServer {
  read: () => RuleConditionsSource;
  save: (input: ConditionsInput, version: number) => Promise<RuleConditionsSource>;
  touch: () => RuleConditionsSource;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function regexProblem(pattern: string): string | null {
  try {
    new RegExp(pattern);
    return null;
  } catch (failure: unknown) {
    return failure instanceof Error ? failure.message : 'does not compile';
  }
}

function leafProblems(condition: Record<string, unknown>, path: string): FieldError[] {
  const problems: FieldError[] = [];
  const { value, op, arg } = condition;
  if (typeof value === 'string' && value.includes('..')) {
    problems.push({ field: `${path}.value`, code: 'json_path_invalid', message: 'is not a valid path' });
  }
  if (op === 'matches' && typeof arg === 'string') {
    const message = regexProblem(arg);
    if (message !== null) {
      problems.push({ field: `${path}.arg`, code: 'regex_invalid', message });
    }
  }
  return problems;
}

export function collectProblems(condition: unknown, path: string): FieldError[] {
  if (!isRecord(condition)) {
    return [];
  }
  const { all, any, not } = condition;
  const list = Array.isArray(all) ? all : Array.isArray(any) ? any : null;
  if (list !== null) {
    const key = Array.isArray(all) ? 'all' : 'any';
    return list.flatMap((item, index) => collectProblems(item, `${path}.${key}.${index}`));
  }
  if (not !== undefined) {
    return collectProblems(not, `${path}.not`);
  }
  return 'op' in condition ? leafProblems(condition, path) : [];
}

function conflict(): ApiError {
  return new ApiError({
    status: 409,
    code: 'version_conflict',
    message: 'The rule changed since it was loaded',
    details: [],
    retryAfter: null,
  });
}

function invalid(details: FieldError[]): ApiError {
  const [first] = details;
  return new ApiError({
    status: 422,
    code: 'validation_failed',
    message: first === undefined ? 'validation failed' : `${first.field} ${first.message}`,
    details,
    retryAfter: null,
  });
}

export function createSampleServer(initial: RuleConditionsSource): SampleServer {
  let current = initial;
  return {
    read: () => current,
    save: (input, version) => {
      if (version !== current.version) {
        return Promise.reject(conflict());
      }
      const problems = collectProblems(input.condition, 'condition');
      if (problems.length > 0) {
        return Promise.reject(invalid(problems));
      }
      current = {
        version: current.version + 1,
        condition: input.condition,
        triggers: input.triggers,
        on_match: input.on_match,
      };
      return Promise.resolve(current);
    },
    touch: () => {
      current = { ...current, version: current.version + 1, on_match: current.on_match === 'next' ? 'stop' : 'next' };
      return current;
    },
  };
}
