import { useEffect, useState } from 'react';
import { checkAliasAvailability, type AliasAvailability } from '../../api/general';

export const ALIAS_CHECK_DEBOUNCE_MS = 400;

export type AliasStatus =
  { state: 'idle' } | { state: 'checking' } | { state: 'error' } | { state: 'ready'; availability: AliasAvailability };

interface AliasAnswer {
  alias: string;
  status: AliasStatus;
}

const IDLE: AliasStatus = { state: 'idle' };
const CHECKING: AliasStatus = { state: 'checking' };

export function useAliasAvailability(candidate: string): AliasStatus {
  const alias = candidate.trim();
  const [answer, setAnswer] = useState<AliasAnswer | null>(null);

  useEffect(() => {
    if (alias === '') {
      return undefined;
    }
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      void checkAliasAvailability(alias, controller.signal).then(
        (availability) => {
          setAnswer({ alias, status: { state: 'ready', availability } });
        },
        () => {
          if (!controller.signal.aborted) {
            setAnswer({ alias, status: { state: 'error' } });
          }
        },
      );
    }, ALIAS_CHECK_DEBOUNCE_MS);
    return () => {
      controller.abort();
      window.clearTimeout(timer);
    };
  }, [alias]);

  if (alias === '') {
    return IDLE;
  }
  if (answer === null || answer.alias !== alias) {
    return CHECKING;
  }
  return answer.status;
}
