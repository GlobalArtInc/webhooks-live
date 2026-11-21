export interface NameValue {
  name: string;
  value: string;
}

export interface CaptureBody {
  encoding: 'utf8' | 'base64';
  data: string;
  truncated: boolean;
}

export interface CaptureFormFile {
  name: string;
  filename: string;
  content_type: string;
  size: number;
}

export interface CaptureForm {
  fields: readonly NameValue[];
  files: readonly CaptureFormFile[];
}

export interface ForwardAttemptInput {
  attempt: number;
  started_at: string;
  duration_ms: number;
  status: number | null;
  error: string | null;
  final_url: string | null;
}

export interface ForwardJobInput {
  id: string;
  kind: 'rule' | 'manual';
  rule_id: string | null;
  target_url: string;
  method: string;
  state: 'pending' | 'running' | 'succeeded' | 'failed';
  attempts: readonly ForwardAttemptInput[];
  created_at: string;
  finished_at: string | null;
}

export type ForwardState = 'pending' | 'succeeded' | 'failed' | null;

export interface CaptureExportInput {
  id: string;
  seq: number;
  endpoint_id: string;
  received_at: string;
  method: string;
  path: string;
  query_string: string;
  client_ip: string;
  content_type: string | null;
  body_size: number;
  status_override: number | null;
  response_status: number;
  forward_state: ForwardState;
  url: string;
  host: string;
  headers: readonly NameValue[];
  query: readonly NameValue[];
  form?: CaptureForm | null;
  body: CaptureBody;
  forwards?: readonly ForwardJobInput[];
}

export interface CurlOptions {
  multiline?: boolean;
  largeBodyPipeThreshold?: number;
  largeBodyFileThreshold?: number;
}

export type SkippedHeaderReason = 'omitted' | 'unsendable';

export interface SkippedHeader {
  name: string;
  value: string;
  reason: SkippedHeaderReason;
}

export interface EndpointResponseConfig {
  status: number;
  content_type: string;
  headers: readonly NameValue[];
  body: CaptureBody;
}

export interface HarOptions {
  endpointResponse?: EndpointResponseConfig;
}

export interface HarNameValue {
  name: string;
  value: string;
}

export interface HarPostData {
  mimeType: string;
  text: string;
  _encoding?: 'base64';
}

export interface HarRequest {
  method: string;
  url: string;
  httpVersion: string;
  cookies: HarNameValue[];
  headers: HarNameValue[];
  queryString: HarNameValue[];
  postData?: HarPostData;
  headersSize: number;
  bodySize: number;
}

export interface HarContent {
  size: number;
  mimeType: string;
  text?: string;
  encoding?: 'base64';
}

export interface HarResponse {
  status: number;
  statusText: string;
  httpVersion: string;
  cookies: HarNameValue[];
  headers: HarNameValue[];
  content: HarContent;
  redirectURL: string;
  headersSize: number;
  bodySize: number;
}

export interface HarTimings {
  send: number;
  wait: number;
  receive: number;
}

export interface HarEntry {
  startedDateTime: string;
  time: number;
  request: HarRequest;
  response: HarResponse;
  cache: Record<string, never>;
  timings: HarTimings;
}

export interface HarCreator {
  name: string;
  version: string;
}

export interface HarLog {
  version: '1.2';
  creator: HarCreator;
  entries: HarEntry[];
}

export interface HarDocument {
  log: HarLog;
}

export interface CaptureJsonBody {
  encoding: 'utf8' | 'base64';
  data: string;
  truncated: boolean;
  size: number;
}

export interface CaptureJsonExport {
  format: 'webhooks.live/capture';
  version: 1;
  id: string;
  seq: number;
  endpoint_id: string;
  received_at: string;
  method: string;
  url: string;
  host: string;
  path: string;
  query_string: string;
  client_ip: string;
  content_type: string | null;
  status_override: number | null;
  response_status: number;
  forward_state: ForwardState;
  headers: NameValue[];
  query: NameValue[];
  form: CaptureForm | null;
  body: CaptureJsonBody;
  forwards: ForwardJobInput[];
}

export type ExportFileKind = 'json' | 'har' | 'body';
