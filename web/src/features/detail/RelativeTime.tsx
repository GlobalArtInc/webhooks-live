import { useEffect, useState } from 'react';
import { formatRelativeTime } from '../../components/format/format';

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

  return <span>{formatRelativeTime(iso, now, locale)}</span>;
}
