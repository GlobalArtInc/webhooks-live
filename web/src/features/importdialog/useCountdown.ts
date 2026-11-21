import { useEffect, useState } from 'react';

const TICK_MS = 1000;

export function useCountdown(initialSeconds: number): number {
  const [remaining, setRemaining] = useState(initialSeconds);

  useEffect(() => {
    if (remaining <= 0) {
      return undefined;
    }
    const timer = window.setTimeout(() => {
      setRemaining(remaining - 1);
    }, TICK_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [remaining]);

  return remaining;
}
