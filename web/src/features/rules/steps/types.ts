import type { VariableEntry } from '../../../components/VariablePicker/variables';

export type StepClass = 'response' | 'compute' | 'state' | 'fetch' | 'effect';

export type StepPhase = 'before' | 'after' | 'any';

export type FieldKind =
  | 'text'
  | 'template'
  | 'number'
  | 'duration'
  | 'boolean'
  | 'select'
  | 'key_value'
  | 'json_path'
  | 'header_name'
  | 'channel'
  | 'secret'
  | 'code';

export type CodeLanguage = 'script' | 'json' | 'text' | 'template';

export type GuardOperator =
  'eq' | 'ne' | 'contains' | 'starts_with' | 'ends_with' | 'matches' | 'exists' | 'gt' | 'lt' | 'in' | 'in_cidr';

export interface KeyValuePair {
  name: string;
  value: string;
}

export type FieldValue = string | number | boolean | null | KeyValuePair[];

export interface SelectOption {
  value: string;
  label: string;
}

export interface StepFieldSchema {
  name: string;
  kind: FieldKind;
  label: string;
  help?: string;
  placeholder?: string;
  required?: boolean;
  nullable?: boolean;
  min?: number;
  max?: number;
  maxLength?: number;
  maxBytes?: number;
  maxRows?: number;
  multiline?: boolean;
  options?: readonly SelectOption[];
  valueKind?: 'text' | 'template';
  nameKind?: 'text' | 'header_name';
  language?: CodeLanguage;
  defaultValue?: FieldValue;
}

export interface StepCatalogueEntry {
  kind: string;
  title: string;
  description: string;
  class: StepClass;
  phase: StepPhase;
  summary: string | null;
  produces: readonly string[];
  fields: readonly StepFieldSchema[];
}

export type GuardArgument = string | number | string[] | null;

export interface GuardPredicate {
  value: string;
  op: GuardOperator;
  arg: GuardArgument;
}

export interface RuleStep {
  id: string;
  kind: string;
  name: string | null;
  continueOnError: boolean;
  guard: GuardPredicate | null;
  fields: Record<string, FieldValue>;
}

export interface StepError {
  stepId: string;
  field: string | null;
  message: string;
}

export interface ChannelOption {
  id: string;
  label: string;
}

export interface StepEditorContext {
  variables: readonly VariableEntry[];
  secrets: readonly string[];
  channels: readonly ChannelOption[];
}

export interface StepRemoval {
  step: RuleStep;
  index: number;
}

export interface StepListHandle {
  focusStep: (stepId: string, field?: string) => void;
}

export type ShapeIssueCode =
  | 'tooLong'
  | 'tooLarge'
  | 'tooSmall'
  | 'tooBig'
  | 'notInteger'
  | 'badHeaderName'
  | 'badJsonPath'
  | 'badName'
  | 'duplicateName'
  | 'tooManyRows'
  | 'guardIncomplete';

export interface ShapeIssue {
  code: ShapeIssueCode;
  limit?: number;
}

export interface StepIssue {
  field: string | null;
  issue: ShapeIssue;
}
