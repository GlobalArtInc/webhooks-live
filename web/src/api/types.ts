export type Timestamp = string;
export type Uuid = string;
export type EndpointId = string;
export type AuthMode = 'open' | 'sso';
export type BodyEncoding = 'utf8' | 'base64';
export type Overflow = 'refuse' | 'ring';
export type EndpointRole = 'owner' | 'editor' | 'viewer';
export type MemberRole = 'viewer' | 'editor';
export type EndpointRoleFilter = 'owner' | 'shared';
export type CaptureOrder = 'desc' | 'asc';
export type ForwardState = 'pending' | 'succeeded' | 'failed';
export type ForwardJobState = 'pending' | 'running' | 'succeeded' | 'failed';
export type ForwardKind = 'rule' | 'manual';
export type ResyncReason = 'lagged' | 'gap' | 'listener_reconnect';
export type EmptyPayload = Record<string, never>;

export interface NameValue {
  name: string;
  value: string;
}

export interface Body {
  encoding: BodyEncoding;
  data: string;
  truncated: boolean;
}

export interface BodyInput {
  encoding: BodyEncoding;
  data: string;
}

export interface User {
  id: Uuid;
  email: string;
  name: string | null;
}

export interface Member {
  user: User;
  role: MemberRole;
  added_at: Timestamp;
}

export interface MemberInput {
  email: string;
  role: MemberRole;
}

export interface ItemList<T> {
  items: T[];
}

export interface Page<T> extends ItemList<T> {
  next_cursor: string | null;
}

export type MemberList = ItemList<Member>;

export type EndpointPage = Page<Endpoint>;

export interface CapturePage extends Page<CaptureSummary> {
  total: number;
}

export interface FieldError {
  field: string;
  code?: string;
  message: string;
}

export interface ErrorDetail {
  code: string;
  message: string;
  fields?: FieldError[];
}

export interface ErrorBody {
  error: ErrorDetail;
}

export interface ResponseConfig {
  status: number;
  content_type: string;
  headers: NameValue[];
  body: Body;
  delay_ms: number;
}

export interface ResponseInput {
  status?: number;
  content_type?: string;
  headers?: NameValue[];
  body?: BodyInput;
  delay_ms?: number;
}

export interface ForwardRule {
  id: Uuid;
  enabled: boolean;
  target_url: string;
  method: string | null;
  content_type: string | null;
  header_allowlist: string[];
  append_path: boolean;
}

export interface ForwardRuleInput {
  id?: Uuid;
  enabled?: boolean;
  target_url: string;
  method?: string | null;
  content_type?: string | null;
  header_allowlist?: string[];
  append_path?: boolean;
}

export interface Endpoint {
  id: EndpointId;
  url: string;
  title: string | null;
  response: ResponseConfig;
  cors: boolean;
  overflow: Overflow;
  forward_rules: ForwardRule[];
  max_captures: number;
  capture_count: number;
  bytes_used: number;
  last_seq: number;
  created_at: Timestamp;
  last_active_at: Timestamp;
  expires_at: Timestamp;
  owner: User | null;
  my_role: EndpointRole | null;
  email?: EndpointEmail;
  dns?: EndpointDns;
}

export interface EndpointInput {
  title?: string | null;
  response?: ResponseInput;
  cors?: boolean;
  overflow?: Overflow;
  forward_rules?: ForwardRuleInput[];
}

export type CaptureKind = 'http' | 'email' | 'dns' | 'schedule';

export interface EndpointEmail {
  enabled: boolean;
  address: string | null;
}

export interface EndpointDns {
  enabled: boolean;
  name: string | null;
}

export interface CaptureSummary {
  id: Uuid;
  seq: number;
  endpoint_id: EndpointId;
  received_at: Timestamp;
  method: string;
  path: string;
  query_string: string;
  client_ip: string;
  content_type: string | null;
  body_size: number;
  status_override: number | null;
  response_status: number;
  forward_state: ForwardState | null;
  kind?: CaptureKind;
  subject?: string | null;
  imported?: boolean;
}

export type CaptureMeta = Readonly<Record<string, unknown>>;

export interface FormFile {
  name: string;
  filename: string;
  content_type: string;
  size: number;
}

export interface CaptureForm {
  fields: NameValue[];
  files: FormFile[];
}

export interface ForwardAttempt {
  attempt: number;
  started_at: Timestamp;
  duration_ms: number;
  status: number | null;
  error: string | null;
  final_url: string | null;
}

export interface ForwardJob {
  id: Uuid;
  kind: ForwardKind;
  rule_id: Uuid | null;
  target_url: string;
  method: string;
  state: ForwardJobState;
  attempts: ForwardAttempt[];
  created_at: Timestamp;
  finished_at: Timestamp | null;
}

export interface ForwardRequest {
  rule_id?: Uuid;
  target_url?: string;
  method?: string | null;
  content_type?: string | null;
  header_allowlist?: string[] | null;
}

export interface Capture extends CaptureSummary {
  url: string;
  host: string;
  headers: NameValue[];
  query: NameValue[];
  form: CaptureForm | null;
  body: Body;
  forwards: ForwardJob[];
  meta?: CaptureMeta | null;
}

export interface CaptureListFilters {
  order?: CaptureOrder;
  limit?: number;
  method?: string;
  kind?: string;
  status?: string;
  q?: string;
  since?: Timestamp;
  until?: Timestamp;
}

export interface CaptureListQuery extends CaptureListFilters {
  cursor?: string | null;
}

export interface EndpointListQuery {
  cursor?: string | null;
  limit?: number;
  role?: EndpointRoleFilter;
}

export type ExportFormat = 'json' | 'ndjson' | 'csv' | 'zip';

export interface CaptureExportQuery extends Pick<CaptureListFilters, 'method' | 'status' | 'q' | 'since' | 'until'> {
  format: ExportFormat;
}

export interface CaptureExportInput {
  format: ExportFormat;
  capture_ids: string[];
}

export interface CaptureExportFile {
  blob: Blob;
  contentDisposition: string | null;
}

export interface CaptureBodyOptions {
  view?: 'text';
  download?: boolean;
}

export interface InstanceAuth {
  provider_name: string | null;
}

export interface InstanceLimits {
  max_captures: number;
  max_body_bytes: number;
  retention_seconds: number;
  max_delay_ms: number;
  max_forward_rules: number;
  max_response_body_bytes: number;
  max_response_headers: number;
}

export interface InstanceFeatures {
  ring_mode_allowed: boolean;
  forwarding: boolean;
  private_forward_targets_allowed: boolean;
}

export interface Instance {
  version: string;
  auth_mode: AuthMode;
  user: User | null;
  public_url: string;
  auth: InstanceAuth;
  limits: InstanceLimits;
  features: InstanceFeatures;
}

export interface StreamReadyPayload {
  endpoint_id: EndpointId;
  last_seq: number;
}

export interface CaptureDeletedPayload {
  capture_id: Uuid;
  seq: number;
}

export interface ForwardResultPayload {
  capture_id: Uuid;
  job: ForwardJob;
}

export interface ResyncPayload {
  reason: ResyncReason;
}

export interface LiveEventPayloads {
  ready: StreamReadyPayload;
  capture: CaptureSummary;
  capture_deleted: CaptureDeletedPayload;
  captures_cleared: EmptyPayload;
  endpoint_updated: Endpoint;
  endpoint_deleted: EmptyPayload;
  forward_result: ForwardResultPayload;
  resync: ResyncPayload;
  bye: EmptyPayload;
}

export type LiveEventName = keyof LiveEventPayloads;

export interface ErrorLimit {
  name: string;
  max: number;
}

export interface SendRequest {
  method: string;
  url: string;
  headers: NameValue[];
  body: BodyInput | null;
  timeout_ms?: number;
  follow_redirects?: boolean;
  endpoint_id?: EndpointId;
  source_capture_id?: Uuid;
}

export interface SendResult {
  id: Uuid;
  status: number | null;
  headers: NameValue[];
  body: Body | null;
  duration_ms: number;
  final_url: string | null;
  error: string | null;
  capture_id: Uuid | null;
}

export interface SentRequest extends SendResult {
  request: Omit<SendRequest, 'body'> & { body: Body | null };
  created_at: Timestamp;
}

export type SentRequestPage = Page<SentRequest>;

export interface SentRequestQuery {
  endpoint_id?: EndpointId;
  cursor?: string | null;
  limit?: number;
}
