import { useEffect, useEffectEvent, useState } from 'react';

export interface PendingState {
  status: 'idle' | 'pending';
}

export interface DoneState<T> {
  status: 'done';
  value: T;
}

export interface FailedState {
  status: 'failed';
  message: string;
}

export type AsyncState<T> = PendingState | DoneState<T> | FailedState;

interface StoredState<T> {
  key: object;
  state: DoneState<T> | FailedState;
}

const IDLE: PendingState = { status: 'idle' };
const PENDING: PendingState = { status: 'pending' };

function describeError(error: unknown): string {
  return error instanceof Error ? error.message : 'Unknown error';
}

export function useAsyncValue<T>(key: object | null, run: (signal: AbortSignal) => Promise<T>): AsyncState<T> {
  const [stored, setStored] = useState<StoredState<T> | null>(null);
  const start = useEffectEvent(run);

  useEffect(() => {
    if (key === null) {
      return undefined;
    }
    let cancelled = false;
    const controller = new AbortController();
    new Promise<T>((resolve) => {
      resolve(start(controller.signal));
    }).then(
      (value) => {
        if (!cancelled) {
          setStored({ key, state: { status: 'done', value } });
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          setStored({ key, state: { status: 'failed', message: describeError(error) } });
        }
      },
    );
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [key]);

  if (key === null) {
    return IDLE;
  }
  if (stored !== null && stored.key === key) {
    return stored.state;
  }
  return PENDING;
}
