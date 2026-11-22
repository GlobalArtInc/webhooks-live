import type { TimeDisplay } from '../../prefs/display';

const MILLISECONDS_PER_SECOND = 1000;
const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3600;
const SECONDS_PER_DAY = 86400;

interface RelativeScale {
  unit: Intl.RelativeTimeFormatUnit;
  seconds: number;
}

const RELATIVE_SCALES: readonly RelativeScale[] = [
  { unit: 'day', seconds: SECONDS_PER_DAY },
  { unit: 'hour', seconds: SECONDS_PER_HOUR },
  { unit: 'minute', seconds: SECONDS_PER_MINUTE },
];

const SECOND_SCALE: RelativeScale = { unit: 'second', seconds: 1 };

const BYTES_PER_STEP = 1024;
const BYTE_UNITS: readonly string[] = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
const ROUNDING_CEILING = BYTES_PER_STEP - 0.05;
const CLOCK_SKEW_TOLERANCE_MS = 5 * 60 * MILLISECONDS_PER_SECOND;

export function formatAbsoluteTime(iso: string, locale: string, utc = false): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) {
    return iso;
  }
  const text = new Intl.DateTimeFormat(locale, {
    dateStyle: 'medium',
    timeStyle: 'medium',
    timeZone: utc ? 'UTC' : undefined,
  }).format(time);
  return utc ? `${text} UTC` : text;
}

export function formatTimestamp(iso: string, now: number, locale: string, display: TimeDisplay): string {
  return display === 'relative'
    ? formatElapsedTime(iso, now, locale)
    : formatAbsoluteTime(iso, locale, display === 'utc');
}

export function formatRelativeTime(iso: string, now: number, locale: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) {
    return iso;
  }
  const difference = Math.round((time - now) / MILLISECONDS_PER_SECOND);
  const scale = RELATIVE_SCALES.find((entry) => Math.abs(difference) >= entry.seconds) ?? SECOND_SCALE;
  const value = Math.round(difference / scale.seconds);
  return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(value, scale.unit);
}

export function formatElapsedTime(iso: string, now: number, locale: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) {
    return iso;
  }
  const ahead = time - now;
  const shown = ahead > 0 && ahead <= CLOCK_SKEW_TOLERANCE_MS ? now : time;
  return formatRelativeTime(new Date(shown).toISOString(), now, locale);
}

export function formatByteSize(bytes: number, locale: string): string {
  let value = bytes;
  let index = 0;
  while (value >= ROUNDING_CEILING && index < BYTE_UNITS.length - 1) {
    value /= BYTES_PER_STEP;
    index += 1;
  }
  const unit = BYTE_UNITS[index] ?? 'B';
  if (index === 0) {
    return `${new Intl.NumberFormat(locale).format(bytes)} ${unit}`;
  }
  const number = new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(value);
  return `${number} ${unit}`;
}
