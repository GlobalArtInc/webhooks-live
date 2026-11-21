import { useEffect, useRef } from 'react';
import { recordEndpointLastSeen } from '../endpoints';

export function useLastSeen(endpointId: string, newestId: string | null, atTop: boolean): void {
  const recorded = useRef<string | null>(null);

  useEffect(() => {
    if (!atTop || newestId === null || recorded.current === newestId) {
      return;
    }
    function record(): void {
      if (newestId !== null && !document.hidden && recorded.current !== newestId) {
        recorded.current = newestId;
        recordEndpointLastSeen(endpointId, newestId);
      }
    }
    record();
    document.addEventListener('visibilitychange', record);
    return () => {
      document.removeEventListener('visibilitychange', record);
    };
  }, [endpointId, newestId, atTop]);
}
