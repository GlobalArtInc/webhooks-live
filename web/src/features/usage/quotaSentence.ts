import type { TFunction } from 'i18next';
import { ApiError } from '../../api/client';
import { isLimitFailure } from '../../api/quota';
import { formatRelativeTime } from '../../components/format/format';
import { meterName } from './meterText';

const MILLISECONDS_PER_SECOND = 1000;
const STORAGE_STATUS = 507;
const COUNT_STATUS = 422;

function retryInstant(error: ApiError, now: number): string | null {
  const resetsAt = error.limit?.resets_at ?? null;
  if (resetsAt !== null) {
    return resetsAt;
  }
  return error.retryAfter === null ? null : new Date(now + error.retryAfter * MILLISECONDS_PER_SECOND).toISOString();
}

export function quotaSentence(error: unknown, t: TFunction, locale: string, now: number = Date.now()): string | null {
  if (!(error instanceof ApiError) || !isLimitFailure(error)) {
    return null;
  }
  if (error.status === STORAGE_STATUS) {
    return t('wl.usage.quota.storage');
  }
  if (error.limit === null) {
    return t('wl.usage.quota.generic');
  }
  const name = meterName(t, error.limit.name);
  if (error.status === COUNT_STATUS) {
    return t('wl.usage.quota.count', { name, max: error.limit.max });
  }
  const when = retryInstant(error, now);
  if (when === null) {
    return t('wl.usage.quota.rateLater', { name });
  }
  return t('wl.usage.quota.rate', { name, when: formatRelativeTime(when, now, locale) });
}
