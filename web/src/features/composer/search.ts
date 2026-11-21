import { isCaptureId, isEndpointId } from '../../app/ids';

export interface ComposeSearch {
  endpoint?: string;
  capture?: string;
}

export interface ComposeTarget {
  to: '/send';
  search: { endpoint: string; capture: string };
}

export function validateComposeSearch(search: Record<string, unknown>): ComposeSearch {
  const endpoint = typeof search.endpoint === 'string' && isEndpointId(search.endpoint) ? search.endpoint : undefined;
  const capture = typeof search.capture === 'string' && isCaptureId(search.capture) ? search.capture : undefined;
  return { endpoint, capture };
}

export function replayCaptureInComposer(captureId: string, endpointId: string): ComposeTarget {
  return { to: '/send', search: { endpoint: endpointId, capture: captureId } };
}
