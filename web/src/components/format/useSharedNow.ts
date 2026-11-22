import { useEffect, useState } from 'react';

export const FAST_TICK_MS = 1000;
export const SLOW_TICK_MS = 15_000;
export const FRESH_WINDOW_MS = 60_000;

interface Subscriber {
  notify: () => void;
  freshest: number | null;
}

const subscribers = new Set<Subscriber>();
let timer: number | null = null;
let timerDelay = 0;
let watchingVisibility = false;

export function nextTickDelay(now: number, list: Iterable<{ freshest: number | null }>): number {
  for (const subscriber of list) {
    if (subscriber.freshest !== null && Math.abs(now - subscriber.freshest) < FRESH_WINDOW_MS) {
      return FAST_TICK_MS;
    }
  }
  return SLOW_TICK_MS;
}

function stopTimer(): void {
  if (timer !== null) {
    window.clearTimeout(timer);
    timer = null;
  }
}

function tick(): void {
  timer = null;
  for (const subscriber of [...subscribers]) {
    subscriber.notify();
  }
  schedule();
}

function schedule(): void {
  if (subscribers.size === 0 || document.hidden) {
    stopTimer();
    return;
  }
  const delay = nextTickDelay(Date.now(), subscribers);
  if (timer !== null && timerDelay === delay) {
    return;
  }
  stopTimer();
  timerDelay = delay;
  timer = window.setTimeout(tick, delay);
}

function handleVisibility(): void {
  if (document.hidden) {
    stopTimer();
    return;
  }
  for (const subscriber of [...subscribers]) {
    subscriber.notify();
  }
  schedule();
}

function subscribe(subscriber: Subscriber): () => void {
  subscribers.add(subscriber);
  if (!watchingVisibility) {
    watchingVisibility = true;
    document.addEventListener('visibilitychange', handleVisibility);
  }
  schedule();
  return () => {
    subscribers.delete(subscriber);
    if (subscribers.size === 0 && watchingVisibility) {
      watchingVisibility = false;
      document.removeEventListener('visibilitychange', handleVisibility);
    }
    schedule();
  };
}

function retarget(subscriber: Subscriber, freshest: number | null): void {
  if (subscriber.freshest === freshest) {
    return;
  }
  subscriber.freshest = freshest;
  subscriber.notify();
  schedule();
}

export function latestTimestamp(times: readonly (string | number)[]): number | null {
  let latest: number | null = null;
  for (const value of times) {
    const parsed = typeof value === 'number' ? value : Date.parse(value);
    if (!Number.isNaN(parsed) && (latest === null || parsed > latest)) {
      latest = parsed;
    }
  }
  return latest;
}

export function useSharedNow(freshest: number | null = null): number {
  const [now, setNow] = useState(Date.now);
  const [subscriber] = useState<Subscriber>(() => ({ notify: () => setNow(Date.now()), freshest }));

  useEffect(() => subscribe(subscriber), [subscriber]);

  useEffect(() => {
    retarget(subscriber, freshest);
  }, [subscriber, freshest]);

  return now;
}

export function sharedSubscriberCount(): number {
  return subscribers.size;
}
