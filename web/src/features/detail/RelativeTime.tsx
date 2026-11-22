import { useMemo } from 'react';
import { formatElapsedTime } from '../../components/format/format';
import { useSharedNow } from '../../components/format/useSharedNow';

interface RelativeTimeProps {
  iso: string;
  locale: string;
}

export function RelativeTime({ iso, locale }: RelativeTimeProps) {
  const time = useMemo(() => Date.parse(iso), [iso]);
  const now = useSharedNow(Number.isNaN(time) ? null : time);

  return <span>{formatElapsedTime(iso, now, locale)}</span>;
}
