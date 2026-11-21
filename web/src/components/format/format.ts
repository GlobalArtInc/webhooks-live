const MILLISECONDS_PER_SECOND = 1000;
const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3600;
const SECONDS_PER_DAY = 86400;

interface RelativeScale {
  unit: Intl.RelativeTimeFormatUnit;
  seconds: number;
}

interface ByteScale {
  unit: 'byte' | 'kilobyte' | 'megabyte' | 'gigabyte';
  size: number;
}

const RELATIVE_SCALES: readonly RelativeScale[] = [
  { unit: 'day', seconds: SECONDS_PER_DAY },
  { unit: 'hour', seconds: SECONDS_PER_HOUR },
  { unit: 'minute', seconds: SECONDS_PER_MINUTE },
];

const SECOND_SCALE: RelativeScale = { unit: 'second', seconds: 1 };

const BYTE_SCALES: readonly ByteScale[] = [
  { unit: 'gigabyte', size: 1e9 },
  { unit: 'megabyte', size: 1e6 },
  { unit: 'kilobyte', size: 1e3 },
];

const PLAIN_BYTE_SCALE: ByteScale = { unit: 'byte', size: 1 };

export function formatAbsoluteTime(iso: string, locale: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) {
    return iso;
  }
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'medium' }).format(time);
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

export function formatByteSize(bytes: number, locale: string): string {
  const scale = BYTE_SCALES.find((entry) => bytes >= entry.size) ?? PLAIN_BYTE_SCALE;
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: scale.unit,
    unitDisplay: 'narrow',
    maximumFractionDigits: 1,
  }).format(bytes / scale.size);
}
