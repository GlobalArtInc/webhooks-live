import { isValidTimezone, normalizeExpression } from './cron';

export type IntervalUnit = 'minutes' | 'hours';

export interface IntervalCadence {
  kind: 'interval';
  every: number;
  unit: IntervalUnit;
}

export interface DailyCadence {
  kind: 'daily';
  time: string;
}

export interface ExpressionCadence {
  kind: 'expression';
  expression: string;
}

export type CadenceValue = IntervalCadence | DailyCadence | ExpressionCadence;

export const DEFAULT_CADENCE: CadenceValue = { kind: 'interval', every: 5, unit: 'minutes' };
export const DEFAULT_TIMEZONE = 'UTC';
export const MAX_MINUTE_INTERVAL = 59;
export const MAX_HOUR_INTERVAL = 23;

const TIME_PATTERN = /^([01]\d|2[0-3]):([0-5]\d)$/;
const MINUTE_INTERVAL_PATTERN = /^\*\/(\d{1,2}) \* \* \* \*$/;
const HOUR_INTERVAL_PATTERN = /^0 \*\/(\d{1,2}) \* \* \*$/;
const DAILY_PATTERN = /^(\d{1,2}) (\d{1,2}) \* \* \*$/;

export function isValidTime(time: string): boolean {
  return TIME_PATTERN.test(time);
}

export function isValidEvery(every: number, unit: IntervalUnit): boolean {
  const max = unit === 'minutes' ? MAX_MINUTE_INTERVAL : MAX_HOUR_INTERVAL;
  return Number.isInteger(every) && every >= 1 && every <= max;
}

export function cadenceToExpression(value: CadenceValue): string {
  switch (value.kind) {
    case 'interval':
      return value.unit === 'minutes' ? `*/${value.every} * * * *` : `0 */${value.every} * * *`;
    case 'daily': {
      const match = TIME_PATTERN.exec(value.time);
      return match === null ? '' : `${Number(match[2])} ${Number(match[1])} * * *`;
    }
    case 'expression':
      return normalizeExpression(value.expression);
  }
}

export function intervalSeconds(every: number, unit: IntervalUnit): number {
  return every * (unit === 'minutes' ? 60 : 3600);
}

export function cronToCadence(expression: string): CadenceValue {
  const normalized = normalizeExpression(expression);
  const minuteMatch = MINUTE_INTERVAL_PATTERN.exec(normalized);
  if (minuteMatch !== null && isValidEvery(Number(minuteMatch[1]), 'minutes')) {
    return { kind: 'interval', every: Number(minuteMatch[1]), unit: 'minutes' };
  }
  const hourMatch = HOUR_INTERVAL_PATTERN.exec(normalized);
  if (hourMatch !== null && isValidEvery(Number(hourMatch[1]), 'hours')) {
    return { kind: 'interval', every: Number(hourMatch[1]), unit: 'hours' };
  }
  const dailyMatch = DAILY_PATTERN.exec(normalized);
  if (dailyMatch !== null && Number(dailyMatch[1]) <= 59 && Number(dailyMatch[2]) <= 23) {
    const time = `${dailyMatch[2]?.padStart(2, '0')}:${dailyMatch[1]?.padStart(2, '0')}`;
    return { kind: 'daily', time };
  }
  return { kind: 'expression', expression: normalized };
}

export function listTimezones(current: string): string[] {
  const supported = typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : [];
  const names = supported.length > 0 ? [...supported] : [DEFAULT_TIMEZONE];
  if (!names.includes(DEFAULT_TIMEZONE)) {
    names.unshift(DEFAULT_TIMEZONE);
  }
  if (isValidTimezone(current) && !names.includes(current)) {
    names.unshift(current);
  }
  return names;
}
