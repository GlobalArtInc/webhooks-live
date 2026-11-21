import { useEffect, useState } from 'react';

const MINUTE_MS = 60_000;

export function useVisibleMinuteNow(): number {
  const [now, setNow] = useState(Date.now);

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (!document.hidden) {
        setNow(Date.now());
      }
    }, MINUTE_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, []);

  return now;
}
