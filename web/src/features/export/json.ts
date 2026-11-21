import { JSON_EXPORT_FORMAT, JSON_EXPORT_VERSION, JSON_INDENT } from './constants';
import type { CaptureExportInput, CaptureJsonExport } from './types';

export function buildJsonExport(capture: CaptureExportInput): CaptureJsonExport {
  return {
    format: JSON_EXPORT_FORMAT,
    version: JSON_EXPORT_VERSION,
    id: capture.id,
    seq: capture.seq,
    endpoint_id: capture.endpoint_id,
    received_at: capture.received_at,
    method: capture.method,
    url: capture.url,
    host: capture.host,
    path: capture.path,
    query_string: capture.query_string,
    client_ip: capture.client_ip,
    content_type: capture.content_type,
    status_override: capture.status_override,
    response_status: capture.response_status,
    forward_state: capture.forward_state,
    headers: capture.headers.map((header) => ({ name: header.name, value: header.value })),
    query: capture.query.map((item) => ({ name: item.name, value: item.value })),
    form: capture.form ?? null,
    body: {
      encoding: capture.body.encoding,
      data: capture.body.data,
      truncated: capture.body.truncated,
      size: capture.body_size,
    },
    forwards: capture.forwards === undefined ? [] : [...capture.forwards],
  };
}

export function toJson(capture: CaptureExportInput): string {
  return JSON.stringify(buildJsonExport(capture), null, JSON_INDENT);
}
