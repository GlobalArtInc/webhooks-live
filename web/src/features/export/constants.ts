import type { ExportFileKind } from './types';

export const OMITTED_REQUEST_HEADERS: readonly string[] = [
  'host',
  'content-length',
  'connection',
  'keep-alive',
  'proxy-connection',
  'proxy-authenticate',
  'proxy-authorization',
  'te',
  'trailer',
  'transfer-encoding',
  'upgrade',
  'accept-encoding',
  'x-request-id',
  'x-endpoint-id',
];

export const OMITTED_REQUEST_HEADER_PREFIXES: readonly string[] = ['x-forwarded-', 'x-envoy-'];

export const HAR_CREATOR_NAME = 'webhooks.live';
export const HAR_CREATOR_VERSION = '1.0.0';
export const HAR_VERSION = '1.2';
export const HAR_HTTP_VERSION = 'HTTP/1.1';
export const DEFAULT_MIME_TYPE = 'application/octet-stream';
export const JSON_EXPORT_FORMAT = 'webhooks.live/capture';
export const JSON_EXPORT_VERSION = 1;
export const JSON_INDENT = 2;
export const DEFAULT_LARGE_BODY_PIPE_THRESHOLD = 64 * 1024;
export const DEFAULT_LARGE_BODY_FILE_THRESHOLD = 256 * 1024;
export const MAX_ESCAPED_CONTROL_CHARS = 8;
export const BODY_METHODS: readonly string[] = ['POST', 'PUT', 'PATCH'];
export const FILE_EXTENSIONS: Readonly<Record<ExportFileKind, string>> = {
  json: 'json',
  har: 'har',
  body: 'bin',
};
