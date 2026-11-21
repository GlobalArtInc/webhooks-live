import { ApiError, API_PREFIX, listInboxCaptures } from '../../api/client';
import type { InboxCaptureQuery } from '../../api/types';

const PROBE_LIMIT = 1;
const NO_RESPONSE_STATUS = 0;
const OK_STATUS = 200;

export function inboxStreamUrl(query: InboxCaptureQuery, lastEventId: string | null): string {
  const params = new URLSearchParams();
  if (query.folder !== undefined) {
    params.set('folder', query.folder);
  }
  if (query.endpoints !== undefined) {
    params.set('endpoints', query.endpoints);
  }
  if (lastEventId !== null && lastEventId !== '') {
    params.set('last_event_id', lastEventId);
  }
  const search = params.toString();
  return search === '' ? `${API_PREFIX}/inbox/stream` : `${API_PREFIX}/inbox/stream?${search}`;
}

export async function probeInbox(query: InboxCaptureQuery): Promise<number> {
  try {
    await listInboxCaptures({ ...query, limit: PROBE_LIMIT });
    return OK_STATUS;
  } catch (error) {
    return error instanceof ApiError ? error.status : NO_RESPONSE_STATUS;
  }
}
