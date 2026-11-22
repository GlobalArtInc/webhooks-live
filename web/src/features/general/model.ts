import type { Endpoint, EndpointInput, Instance, RetentionMode } from '../../api/types';

const HOUR_SECONDS = 3600;
const DAY_SECONDS = 86400;
const WEEK_SECONDS = 604800;
const MONTH_SECONDS = 2592000;
const MIN_CAPTURE_TTL_SECONDS = 60;
const NEAR_LIMIT_RATIO = 0.9;

export const MIN_SLIDING_SECONDS = HOUR_SECONDS;

export type LifetimeLabelKey =
  | 'wl.general.lifetime.preset.hour'
  | 'wl.general.lifetime.preset.day'
  | 'wl.general.lifetime.preset.week'
  | 'wl.general.lifetime.preset.month'
  | 'wl.general.lifetime.preset.custom';

export interface DurationOption {
  seconds: number | null;
  label: LifetimeLabelKey | 'wl.general.lifetime.preset.off';
}

export interface GeneralDraft {
  title: string;
  tags: string[];
  retentionMode: RetentionMode;
  retentionSeconds: number | null;
  maxCaptures: string;
  captureTtl: number | null;
}

export interface GeneralLimits {
  defaultRetentionSeconds: number;
  maxRetentionSeconds: number;
  maxCaptures: number;
  neverAllowed: boolean;
}

export type MeterState = 'unlimited' | 'normal' | 'near' | 'full';

export type UsageLabelKey =
  | 'wl.general.usage.meter.endpoints'
  | 'wl.general.usage.meter.storageBytes'
  | 'wl.general.usage.meter.outbound'
  | 'wl.general.usage.meter.notifications'
  | 'wl.general.usage.meter.emailNotifications'
  | 'wl.general.usage.meter.exports'
  | 'wl.general.usage.meter.imports'
  | 'wl.general.usage.meter.composer'
  | 'wl.general.usage.meter.schedules'
  | 'wl.general.usage.meter.aliases'
  | 'wl.general.usage.meter.tokens'
  | 'wl.general.usage.meter.neverExpire';

const LIFETIME_PRESETS: readonly DurationOption[] = [
  { seconds: HOUR_SECONDS, label: 'wl.general.lifetime.preset.hour' },
  { seconds: DAY_SECONDS, label: 'wl.general.lifetime.preset.day' },
  { seconds: WEEK_SECONDS, label: 'wl.general.lifetime.preset.week' },
  { seconds: MONTH_SECONDS, label: 'wl.general.lifetime.preset.month' },
];

const CAPTURE_TTL_PRESETS: readonly DurationOption[] = [
  { seconds: null, label: 'wl.general.lifetime.preset.off' },
  { seconds: HOUR_SECONDS, label: 'wl.general.lifetime.preset.hour' },
  { seconds: DAY_SECONDS, label: 'wl.general.lifetime.preset.day' },
  { seconds: WEEK_SECONDS, label: 'wl.general.lifetime.preset.week' },
];

const USAGE_LABEL_KEYS: Readonly<Record<string, UsageLabelKey>> = {
  endpoints: 'wl.general.usage.meter.endpoints',
  storage_bytes: 'wl.general.usage.meter.storageBytes',
  outbound: 'wl.general.usage.meter.outbound',
  notifications: 'wl.general.usage.meter.notifications',
  email_notifications: 'wl.general.usage.meter.emailNotifications',
  exports: 'wl.general.usage.meter.exports',
  imports: 'wl.general.usage.meter.imports',
  composer: 'wl.general.usage.meter.composer',
  schedules: 'wl.general.usage.meter.schedules',
  aliases: 'wl.general.usage.meter.aliases',
  tokens: 'wl.general.usage.meter.tokens',
  never_expire: 'wl.general.usage.meter.neverExpire',
};

export function limitsOf(instance: Instance | undefined): GeneralLimits {
  const limits = instance?.limits;
  const defaultRetentionSeconds = limits?.retention_seconds ?? MIN_SLIDING_SECONDS;
  return {
    defaultRetentionSeconds,
    maxRetentionSeconds: limits?.max_retention_seconds ?? defaultRetentionSeconds,
    maxCaptures: limits?.max_captures_settable ?? limits?.max_captures ?? 1,
    neverAllowed: Boolean(instance?.user) && (limits?.never_expire_endpoints ?? 0) > 0,
  };
}

export function isEndpointOwner(endpoint: Endpoint): boolean {
  return endpoint.my_role === 'owner' || (endpoint.my_role === null && endpoint.owner === null);
}

export function draftOf(endpoint: Endpoint): GeneralDraft {
  return {
    title: endpoint.title ?? '',
    tags: [...endpoint.tags],
    retentionMode: endpoint.retention?.mode ?? 'sliding',
    retentionSeconds: endpoint.retention?.seconds ?? null,
    maxCaptures: String(endpoint.max_captures),
    captureTtl: endpoint.capture_ttl_seconds ?? null,
  };
}

export function lifetimeOptions(maxSeconds: number, current: number | null): DurationOption[] {
  const fitting = LIFETIME_PRESETS.filter((option) => option.seconds !== null && option.seconds <= maxSeconds);
  return withCustom(fitting, current);
}

export function captureTtlOptions(current: number | null): DurationOption[] {
  return withCustom(CAPTURE_TTL_PRESETS, current);
}

function withCustom(options: readonly DurationOption[], current: number | null): DurationOption[] {
  if (current === null || options.some((option) => option.seconds === current)) {
    return [...options];
  }
  return [...options, { seconds: current, label: 'wl.general.lifetime.preset.custom' }];
}

export function isLifetimeValid(draft: GeneralDraft, limits: GeneralLimits): boolean {
  if (draft.retentionMode === 'never') {
    return true;
  }
  const seconds = draft.retentionSeconds ?? limits.defaultRetentionSeconds;
  return seconds >= MIN_SLIDING_SECONDS && seconds <= limits.maxRetentionSeconds;
}

export function isMaxCapturesValid(value: string, limits: GeneralLimits): boolean {
  if (!/^\d+$/.test(value)) {
    return false;
  }
  const parsed = Number(value);
  return parsed >= 1 && parsed <= limits.maxCaptures;
}

export function isCaptureTtlValid(seconds: number | null): boolean {
  return seconds === null || seconds >= MIN_CAPTURE_TTL_SECONDS;
}

export function patchFor(draft: GeneralDraft, source: Endpoint, isOwner: boolean): EndpointInput {
  const trimmed = draft.title.trim();
  const input: EndpointInput = { title: trimmed === '' ? null : trimmed, tags: [...draft.tags] };
  if (!isOwner) {
    return input;
  }
  const base = draftOf(source);
  if (draft.retentionMode !== base.retentionMode || draft.retentionSeconds !== base.retentionSeconds) {
    input.retention =
      draft.retentionMode === 'never'
        ? { mode: 'never', seconds: null }
        : { mode: 'sliding', seconds: draft.retentionSeconds };
  }
  if (draft.maxCaptures !== base.maxCaptures) {
    input.max_captures = Number(draft.maxCaptures);
  }
  if (draft.captureTtl !== base.captureTtl) {
    input.capture_ttl_seconds = draft.captureTtl;
  }
  return input;
}

export function meterState(used: number, max: number | null): MeterState {
  if (max === null) {
    return 'unlimited';
  }
  if (used >= max) {
    return 'full';
  }
  return used >= max * NEAR_LIMIT_RATIO ? 'near' : 'normal';
}

export function usageLabelKey(name: string): UsageLabelKey | null {
  return USAGE_LABEL_KEYS[name] ?? null;
}
