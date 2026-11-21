import type { NameValue, Timestamp, Uuid } from './types';

export type RuleOperator =
  'eq' | 'ne' | 'contains' | 'starts_with' | 'ends_with' | 'matches' | 'exists' | 'gt' | 'lt' | 'in' | 'in_cidr';

export type RuleConditionArg = string | number | string[] | null;

export interface RuleLeafCondition {
  value: string;
  op: RuleOperator;
  arg: RuleConditionArg;
}

export interface RuleAllCondition {
  all: RuleCondition[];
}

export interface RuleAnyCondition {
  any: RuleCondition[];
}

export interface RuleNotCondition {
  not: RuleCondition;
}

export type RuleEmptyCondition = Record<string, never>;

export type RuleCondition =
  RuleEmptyCondition | RuleAllCondition | RuleAnyCondition | RuleNotCondition | RuleLeafCondition;

export interface RuleStepBase {
  name?: string | null;
  continue_on_error?: boolean;
}

export interface RespondStep extends RuleStepBase {
  kind: 'respond';
  status: string;
  content_type: string | null;
  headers: NameValue[];
  body: string;
}

export interface SetHeaderStep extends RuleStepBase {
  kind: 'set_header';
  name: string;
  value: string;
}

export interface RemoveHeaderStep extends RuleStepBase {
  kind: 'remove_header';
  name: string;
}

export interface DelayStep extends RuleStepBase {
  kind: 'delay';
  ms: number;
}

export interface StopStep extends RuleStepBase {
  kind: 'stop';
}

export interface ExtractStep extends RuleStepBase {
  kind: 'extract';
  from: string;
  json_path?: string;
  regex?: string;
  group?: number;
  into: string;
}

export interface SetVarStep extends RuleStepBase {
  kind: 'set_var';
  into: string;
  value: string;
}

export interface TransformStep extends RuleStepBase {
  kind: 'transform';
  into: string;
  template: string;
}

export interface SetStoreStep extends RuleStepBase {
  kind: 'set_store';
  name: string;
  value: string;
}

export interface IncrementStep extends RuleStepBase {
  kind: 'increment';
  counter: string;
  by?: number;
}

export interface ScriptStep extends RuleStepBase {
  kind: 'script';
  source: string;
}

export interface HttpStep extends RuleStepBase {
  kind: 'http';
  method: string;
  url: string;
  headers: NameValue[];
  body: string | null;
  timeout_ms?: number;
  follow_redirects?: boolean;
}

export interface ForwardStep extends RuleStepBase {
  kind: 'forward';
  forward_rule_id?: Uuid;
  target_url?: string;
  method?: string;
  content_type?: string;
  header_allowlist?: string[];
  body_from?: 'capture' | 'payload';
}

export interface NotifyStep extends RuleStepBase {
  kind: 'notify';
  channel_id: Uuid;
  message: string | null;
}

export interface StoreS3Step extends RuleStepBase {
  kind: 'store_s3';
  bucket_url: string;
  prefix: string;
  region: string;
  access_key_secret: string;
  secret_key_secret: string;
  content: 'capture_json' | 'body';
}

export type RuleStep =
  | RespondStep
  | SetHeaderStep
  | RemoveHeaderStep
  | DelayStep
  | StopStep
  | ExtractStep
  | SetVarStep
  | TransformStep
  | SetStoreStep
  | IncrementStep
  | ScriptStep
  | HttpStep
  | ForwardStep
  | NotifyStep
  | StoreS3Step;

export type RuleStepKind = RuleStep['kind'];

export interface Rule {
  id: Uuid;
  name: string;
  enabled: boolean;
  condition: RuleCondition;
  steps: RuleStep[];
  position: number;
  created_at: Timestamp;
  updated_at: Timestamp;
}

export interface RuleInput {
  name?: string;
  enabled?: boolean;
  condition?: RuleCondition;
  steps?: RuleStep[];
}

export interface RuleList {
  items: Rule[];
  version: number;
}

export interface RuleOrderInput {
  ids: Uuid[];
}
