import { useParams, useRouter, useSearch } from '@tanstack/react-router';
import { useEffect, useState } from 'react';
import { ApiError } from '../../api/client';
import { unlockEndpoint } from './accessApi';
import { storeGrant } from './grants';
import { LockedPage, type LockedState } from './LockedPage';

const UNLOCK_ROUTE_ID = '/shell/e/$id/unlock';
const DEFAULT_RETRY_SECONDS = 30;
const TICK_MS = 1000;
const WRONG_STATUSES: readonly number[] = [401, 404, 410];

export function UnlockScreen() {
  const { id: endpointId } = useParams({ from: UNLOCK_ROUTE_ID });
  const { next } = useSearch({ from: UNLOCK_ROUTE_ID });
  const [state, setState] = useState<LockedState>('idle');
  const [secondsLeft, setSecondsLeft] = useState<number | null>(null);
  const router = useRouter();
  const shownState: LockedState = state === 'limited' && secondsLeft === 0 ? 'idle' : state;

  useEffect(() => {
    if (secondsLeft === null || secondsLeft <= 0) {
      return undefined;
    }
    const timer = window.setTimeout(() => {
      setSecondsLeft((current) => (current === null ? null : Math.max(0, current - 1)));
    }, TICK_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [secondsLeft]);

  async function unlock(password: string): Promise<void> {
    setState('checking');
    setSecondsLeft(null);
    try {
      const grant = await unlockEndpoint(endpointId, password);
      if (!storeGrant(endpointId, grant.grant, grant.expires_at)) {
        throw new TypeError('The unlock answer is malformed.');
      }
      router.history.replace(next ?? `/e/${endpointId}`);
    } catch (failure: unknown) {
      if (failure instanceof ApiError && failure.status === 429) {
        setSecondsLeft(failure.retryAfter ?? DEFAULT_RETRY_SECONDS);
        setState('limited');
      } else if (failure instanceof ApiError && WRONG_STATUSES.includes(failure.status)) {
        setState('wrong');
      } else {
        setState('failed');
      }
    }
  }

  return (
    <LockedPage
      state={shownState}
      secondsLeft={secondsLeft}
      onUnlock={(password) => {
        void unlock(password);
      }}
    />
  );
}
