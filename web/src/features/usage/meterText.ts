import type { TFunction } from 'i18next';
import type { UsageMeter, UsageWindow } from '../../api/usage';
import { formatByteSize, formatRelativeTime } from '../../components/format/format';
import { meterLevel, type MeterLevel } from './meterLevel';

const STORAGE_METER = 'storage_bytes';

export interface HeaderWarning {
  text: string;
  level: Exclude<MeterLevel, 'normal'>;
}

export function meterName(t: TFunction, name: string): string {
  switch (name) {
    case 'endpoints':
      return t('wl.usage.name.endpoints');
    case 'storage_bytes':
      return t('wl.usage.name.storage');
    case 'exports':
      return t('wl.usage.name.exports');
    case 'imports':
      return t('wl.usage.name.imports');
    case 'outbound':
      return t('wl.usage.name.outbound');
    case 'notifications':
      return t('wl.usage.name.notifications');
    case 'email_notifications':
      return t('wl.usage.name.emailNotifications');
    case 'schedules':
      return t('wl.usage.name.schedules');
    case 'tokens':
      return t('wl.usage.name.tokens');
    case 'folders':
      return t('wl.usage.name.folders');
    case 'rules':
      return t('wl.usage.name.rules');
    case 'composer':
      return t('wl.usage.name.composer');
    case 'aliases':
      return t('wl.usage.name.aliases');
    case 'never_expire':
      return t('wl.usage.name.neverExpire');
    default:
      return name;
  }
}

export function windowLabel(t: TFunction, window: UsageWindow): string {
  switch (window) {
    case 'hour':
      return t('wl.usage.window.hour');
    case 'day':
      return t('wl.usage.window.day');
    case 'total':
      return t('wl.usage.window.total');
  }
}

export function formatAmount(meter: UsageMeter, amount: number, locale: string): string {
  if (meter.name === STORAGE_METER) {
    return formatByteSize(amount, locale);
  }
  return new Intl.NumberFormat(locale).format(amount);
}

export function formatPercent(used: number, max: number, locale: string): string {
  const ratio = max > 0 ? used / max : 1;
  return new Intl.NumberFormat(locale, { style: 'percent', maximumFractionDigits: 0 }).format(ratio);
}

export function atLimitText(
  t: TFunction,
  meter: UsageMeter,
  level: MeterLevel,
  locale: string,
  now: number,
): string | null {
  if (level === 'normal') {
    return null;
  }
  if (level === 'near') {
    return t('wl.usage.state.near', { percent: formatPercent(meter.used, meter.max, locale) });
  }
  if (meter.window === 'total') {
    return t('wl.usage.state.fullTotal');
  }
  if (meter.resets_at === null) {
    return t('wl.usage.state.fullOpen');
  }
  return t('wl.usage.state.fullWindow', { when: formatRelativeTime(meter.resets_at, now, locale) });
}

export function headerWarning(t: TFunction, meters: readonly UsageMeter[], locale: string): HeaderWarning | null {
  const full = meters.find((meter) => meterLevel(meter) === 'full');
  if (full !== undefined) {
    return { text: t('wl.usage.header.full', { name: meterName(t, full.name) }), level: 'full' };
  }
  const near = meters.find((meter) => meterLevel(meter) === 'near');
  if (near !== undefined) {
    const percent = formatPercent(near.used, near.max, locale);
    return { text: t('wl.usage.header.near', { name: meterName(t, near.name), percent }), level: 'near' };
  }
  return null;
}
