export const BACKOFF_MIN_MS = 1000;
export const BACKOFF_MAX_MS = 30_000;
export const BACKOFF_JITTER = 0.2;
export const STABLE_CONNECTION_MS = 30_000;
export const HIDDEN_GRACE_MS = 60_000;
export const BYE_LOOP_GUARD_MS = 2000;

export function backoffDelay(attempt: number, random: number): number {
  const base = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** Math.max(0, attempt));
  const jittered = base * (1 + BACKOFF_JITTER * (2 * random - 1));
  return Math.round(Math.min(BACKOFF_MAX_MS, Math.max(BACKOFF_MIN_MS, jittered)));
}
