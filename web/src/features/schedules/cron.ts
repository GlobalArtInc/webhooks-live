export interface CronSpec {
  minutes: ReadonlySet<number>;
  hours: ReadonlySet<number>;
  daysOfMonth: ReadonlySet<number>;
  daysOfMonthRestricted: boolean;
  months: ReadonlySet<number>;
  weekdays: ReadonlySet<number>;
  weekdaysRestricted: boolean;
}

export interface WallClock {
  month: number;
  day: number;
  hour: number;
  minute: number;
  weekday: number;
}

const FIELD_COUNT = 5;
const PART_PATTERN = /^(\*|\d+(?:-\d+)?)(?:\/(\d+))?$/;
const WEEKDAY_NAMES = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const MINUTE_MS = 60_000;
const MINUTES_PER_DAY = 1440;
const MAX_STEPS = 20_000;
const HORIZON_DAYS = 366 * 5;
const formatters = new Map<string, Intl.DateTimeFormat>();

interface FieldRange {
  min: number;
  max: number;
}

const MINUTE_RANGE: FieldRange = { min: 0, max: 59 };
const HOUR_RANGE: FieldRange = { min: 0, max: 23 };
const DAY_RANGE: FieldRange = { min: 1, max: 31 };
const MONTH_RANGE: FieldRange = { min: 1, max: 12 };
const WEEKDAY_RANGE: FieldRange = { min: 0, max: 7 };

function expandPart(part: string, range: FieldRange): number[] | null {
  const match = PART_PATTERN.exec(part);
  if (match === null) {
    return null;
  }
  const [, body = '', stepText] = match;
  const step = stepText === undefined ? 1 : Number(stepText);
  if (step < 1) {
    return null;
  }
  let start: number;
  let end: number;
  if (body === '*') {
    start = range.min;
    end = range.max;
  } else if (body.includes('-')) {
    const [first = '', last = ''] = body.split('-');
    start = Number(first);
    end = Number(last);
  } else {
    start = Number(body);
    end = stepText === undefined ? start : range.max;
  }
  if (start < range.min || end > range.max || start > end) {
    return null;
  }
  const values: number[] = [];
  for (let value = start; value <= end; value += step) {
    values.push(value);
  }
  return values;
}

function parseField(text: string, range: FieldRange): Set<number> | null {
  const values = new Set<number>();
  for (const part of text.split(',')) {
    const expanded = expandPart(part, range);
    if (expanded === null) {
      return null;
    }
    for (const value of expanded) {
      values.add(value);
    }
  }
  return values.size === 0 ? null : values;
}

export function normalizeExpression(expression: string): string {
  return expression.trim().replace(/\s+/g, ' ');
}

export function parseCron(expression: string): CronSpec | null {
  const fields = normalizeExpression(expression).split(' ');
  if (fields.length !== FIELD_COUNT) {
    return null;
  }
  const [minuteText = '', hourText = '', dayText = '', monthText = '', weekdayText = ''] = fields;
  const minutes = parseField(minuteText, MINUTE_RANGE);
  const hours = parseField(hourText, HOUR_RANGE);
  const daysOfMonth = parseField(dayText, DAY_RANGE);
  const months = parseField(monthText, MONTH_RANGE);
  const rawWeekdays = parseField(weekdayText, WEEKDAY_RANGE);
  if (minutes === null || hours === null || daysOfMonth === null || months === null || rawWeekdays === null) {
    return null;
  }
  const weekdays = new Set([...rawWeekdays].map((value) => value % 7));
  return {
    minutes,
    hours,
    daysOfMonth,
    daysOfMonthRestricted: !dayText.startsWith('*'),
    months,
    weekdays,
    weekdaysRestricted: !weekdayText.startsWith('*'),
  };
}

export function isValidTimezone(timezone: string): boolean {
  try {
    formatterFor(timezone);
    return true;
  } catch {
    return false;
  }
}

function formatterFor(timezone: string): Intl.DateTimeFormat {
  const cached = formatters.get(timezone);
  if (cached !== undefined) {
    return cached;
  }
  const formatter = new Intl.DateTimeFormat('en-US', {
    timeZone: timezone,
    hourCycle: 'h23',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: 'numeric',
    weekday: 'short',
  });
  formatters.set(timezone, formatter);
  return formatter;
}

export function wallClockAt(instant: number, timezone: string): WallClock {
  const parts = formatterFor(timezone).formatToParts(instant);
  const read = (type: Intl.DateTimeFormatPartTypes): string => parts.find((part) => part.type === type)?.value ?? '';
  return {
    month: Number(read('month')),
    day: Number(read('day')),
    hour: Number(read('hour')),
    minute: Number(read('minute')),
    weekday: WEEKDAY_NAMES.indexOf(read('weekday')),
  };
}

function dayMatches(spec: CronSpec, clock: WallClock): boolean {
  if (!spec.months.has(clock.month)) {
    return false;
  }
  const dayOk = spec.daysOfMonth.has(clock.day);
  const weekdayOk = spec.weekdays.has(clock.weekday);
  if (spec.daysOfMonthRestricted && spec.weekdaysRestricted) {
    return dayOk || weekdayOk;
  }
  return dayOk && weekdayOk;
}

export function nextRuns(expression: string, timezone: string, from: number, count: number): number[] | null {
  const spec = parseCron(expression);
  if (spec === null || !isValidTimezone(timezone)) {
    return null;
  }
  const runs: number[] = [];
  const horizon = from + HORIZON_DAYS * MINUTES_PER_DAY * MINUTE_MS;
  let instant = (Math.floor(from / MINUTE_MS) + 1) * MINUTE_MS;
  for (let step = 0; step < MAX_STEPS && runs.length < count && instant < horizon; step += 1) {
    const clock = wallClockAt(instant, timezone);
    if (!dayMatches(spec, clock)) {
      instant += (MINUTES_PER_DAY - clock.hour * 60 - clock.minute) * MINUTE_MS;
    } else if (!spec.hours.has(clock.hour)) {
      instant += (60 - clock.minute) * MINUTE_MS;
    } else if (!spec.minutes.has(clock.minute)) {
      instant += MINUTE_MS;
    } else {
      runs.push(instant);
      instant += MINUTE_MS;
    }
  }
  return runs;
}

export function formatRunTime(instant: number, locale: string, timezone: string): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
    timeZone: timezone,
  }).format(instant);
}
