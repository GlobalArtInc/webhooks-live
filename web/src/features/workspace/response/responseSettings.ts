import type { BodyEncoding, Endpoint, EndpointInput, Overflow } from '../../../api/types';
import { createHeaderRow, type ResponseDraft } from '../../endpoints/responseDraft';

export interface ResponseSettingsDraft extends ResponseDraft {
  bodyEncoding: BodyEncoding;
  overflow: Overflow;
  templated: boolean;
}

export function responseSettingsDraftOf(endpoint: Endpoint): ResponseSettingsDraft {
  return {
    status: String(endpoint.response.status),
    contentType: endpoint.response.content_type,
    body: endpoint.response.body.data,
    bodyEncoding: endpoint.response.body.encoding,
    delay: String(endpoint.response.delay_ms),
    cors: endpoint.cors,
    overflow: endpoint.overflow,
    templated: endpoint.response.templated === true,
    headers: endpoint.response.headers.map((header) => ({
      ...createHeaderRow(),
      name: header.name,
      value: header.value,
    })),
  };
}

export function serializeResponseSettings(draft: ResponseSettingsDraft): string {
  return JSON.stringify({ ...draft, headers: draft.headers.map(({ name, value }) => ({ name, value })) });
}

export function responseSettingsInput(draft: ResponseSettingsDraft): EndpointInput {
  return {
    cors: draft.cors,
    overflow: draft.overflow,
    response: {
      status: Number(draft.status.trim()),
      content_type: draft.contentType.trim(),
      body: { encoding: draft.bodyEncoding, data: draft.body },
      delay_ms: Number(draft.delay.trim()),
      templated: draft.templated,
      headers: draft.headers.map((row) => ({ name: row.name.trim(), value: row.value.trim() })),
    },
  };
}
