import type { MyEndpoint } from './storage';

const SHORT_ID_HEAD = 6;
const SHORT_ID_TAIL = 3;
const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const SECOND_MS = 1000;

interface RelativeUnit {
  unit: Intl.RelativeTimeFormatUnit;
  size: number;
}

const RELATIVE_UNITS: readonly RelativeUnit[] = [
  { unit: 'day', size: DAY_MS },
  { unit: 'hour', size: HOUR_MS },
  { unit: 'minute', size: MINUTE_MS },
  { unit: 'second', size: SECOND_MS },
];

export function shortEndpointId(id: string): string {
  return `${id.slice(0, SHORT_ID_HEAD)}...${id.slice(-SHORT_ID_TAIL)}`;
}

export function knownEndpointTitle(entry: MyEndpoint, cachedTitle: string | null): string | null {
  return cachedTitle ?? entry.title ?? null;
}

export function endpointName(entry: MyEndpoint, cachedTitle: string | null = null): string {
  return entry.label ?? knownEndpointTitle(entry, cachedTitle) ?? shortEndpointId(entry.id);
}

export function formatRelativeTime(then: number, now: number, locale: string): string {
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  const elapsed = Math.max(0, now - then);
  for (const { unit, size } of RELATIVE_UNITS) {
    if (elapsed >= size) {
      return formatter.format(-Math.floor(elapsed / size), unit);
    }
  }
  return formatter.format(0, 'second');
}
