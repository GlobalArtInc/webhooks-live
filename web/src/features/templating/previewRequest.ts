import { STATUS_MAX, STATUS_MIN } from '../endpoints/responseDraft';
import type { ResponseSettingsDraft } from '../workspace/response/responseSettings';
import type { PreviewRequest } from './types';

const FALLBACK_STATUS = 200;
const INTEGER_PATTERN = /^\d+$/;

function previewStatus(text: string): number {
  const value = text.trim();
  if (!INTEGER_PATTERN.test(value)) {
    return FALLBACK_STATUS;
  }
  const status = Number(value);
  return status >= STATUS_MIN && status <= STATUS_MAX ? status : FALLBACK_STATUS;
}

export function previewRequestOf(draft: ResponseSettingsDraft, captureId: string | null): PreviewRequest {
  return {
    capture_id: captureId,
    response: {
      status: previewStatus(draft.status),
      content_type: draft.contentType.trim(),
      headers: draft.headers.map((row) => ({ name: row.name.trim(), value: row.value.trim() })),
      body: { encoding: draft.bodyEncoding, data: draft.body },
    },
  };
}
