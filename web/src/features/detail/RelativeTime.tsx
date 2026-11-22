import { useEffect, useState } from 'react';
import { formatElapsedTime } from '../../components/format/format';

const TICK_MS = 30000;

interface RelativeTimeProps {
  iso: string;
  locale: string;
}

export function RelativeTime({ iso, locale }: RelativeTimeProps) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (!document.hidden) {
        setNow(Date.now());
      }
    }, TICK_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, []);

  return <span>{formatElapsedTime(iso, now, locale)}</span>;
}
