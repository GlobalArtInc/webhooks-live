import type { UsageMeter } from '../../api/usage';

export type MeterLevel = 'normal' | 'near' | 'full';

const WHOLE_PERCENT = 100;
const NEAR_PERCENT = 90;

export function meterLevel(meter: UsageMeter): MeterLevel {
  if (meter.used >= meter.max) {
    return 'full';
  }
  return meter.used * WHOLE_PERCENT >= meter.max * NEAR_PERCENT ? 'near' : 'normal';
}

export function fillPercent(meter: UsageMeter): number {
  if (meter.max <= 0) {
    return WHOLE_PERCENT;
  }
  return Math.min(WHOLE_PERCENT, (meter.used / meter.max) * WHOLE_PERCENT);
}
