import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import type { ConnectionState, EndReason } from '../live/types';
import { LiveSource } from '../live/LiveSource';
import { inboxListKey } from './cache';
import { InboxSession } from './inboxSession';
import { queryForKey } from './scope';
import { inboxStreamUrl, probeInbox } from './stream';

export interface InboxStreamState {
  scopeKey: string | null;
  connection: ConnectionState;
  endReason: EndReason | null;
  lastEventAt: number | null;
}

const IDLE: InboxStreamState = { scopeKey: null, connection: 'connecting', endReason: null, lastEventAt: null };

export function useInboxStream(scopeKey: string | null): InboxStreamState {
  const queryClient = useQueryClient();
  const [state, setState] = useState<InboxStreamState>(IDLE);

  useEffect(() => {
    if (scopeKey === null) {
      return undefined;
    }
    const query = queryForKey(scopeKey);
    if (query === null) {
      return undefined;
    }
    const source = new LiveSource({
      buildUrl: (lastEventId) => inboxStreamUrl(query, lastEventId),
      probe: () => probeInbox(query),
      finishOnEndpointDeleted: false,
    });
    const session = new InboxSession({
      queryClient,
      queryKey: inboxListKey(scopeKey),
      source,
      onActivity: () => {
        setState((current) => ({ ...current, scopeKey, lastEventAt: Date.now() }));
      },
    });
    const unsubscribe = source.on('state', (change) => {
      setState((current) => ({
        ...current,
        scopeKey,
        connection: change.state,
        endReason: change.reason,
      }));
    });
    session.start();
    return () => {
      unsubscribe();
      session.dispose();
    };
  }, [queryClient, scopeKey]);

  return state.scopeKey === scopeKey ? state : { ...IDLE, scopeKey };
}
