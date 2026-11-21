export type TriggerKind = 'http' | 'email' | 'dns';

export type OnMatch = 'next' | 'stop';

export type ConditionMode = 'all' | 'any';

export type ConditionArg = string | number | string[] | null;

export interface LeafCondition {
  value: string;
  op: string;
  arg?: ConditionArg;
}

export interface AllCondition {
  all: ApiCondition[];
}

export interface AnyCondition {
  any: ApiCondition[];
}

export interface NotCondition {
  not: ApiCondition;
}

export type EmptyCondition = Record<string, never>;

export type ApiCondition = EmptyCondition | AllCondition | AnyCondition | NotCondition | LeafCondition;

export type ValueType = 'string' | 'number' | 'ip';

export type ParameterKind = 'header_name' | 'json_path' | 'name' | 'variable';

export interface ParameterEntry {
  kind: ParameterKind;
}

export interface SubjectEntry {
  id: string;
  label: string;
  prefix: string;
  kinds: readonly TriggerKind[];
  value_type: ValueType;
  parameter: ParameterEntry | null;
  suggestions?: readonly string[];
}

export type OperatorArgKind = 'none' | 'value' | 'number' | 'regex' | 'list' | 'cidr_list';

export interface OperatorEntry {
  id: string;
  label: string;
  value_types: readonly ValueType[];
  arg: OperatorArgKind;
}

export interface ConditionCatalogue {
  subjects: readonly SubjectEntry[];
  operators: readonly OperatorEntry[];
}

export interface ConditionLimits {
  maxDepth: number;
  maxLeaves: number;
}

export interface ConditionsInput {
  condition: ApiCondition;
  triggers: TriggerKind[];
  on_match: OnMatch;
}

export interface RuleConditionsSource {
  version: number;
  condition: unknown;
  triggers: readonly string[];
  on_match: string;
}

export interface LeafDraft {
  kind: 'leaf';
  id: string;
  negate: boolean;
  subject: string;
  parameter: string;
  op: string;
  argText: string;
  argList: string[];
}

export interface GroupDraft {
  kind: 'group';
  id: string;
  negate: boolean;
  mode: ConditionMode;
  children: NodeDraft[];
}

export type NodeDraft = LeafDraft | GroupDraft;

export interface ConditionsDraft {
  triggers: TriggerKind[];
  root: GroupDraft;
  onMatch: OnMatch;
}

export type RowPart = 'subject' | 'parameter' | 'operator' | 'arg' | 'row';

export interface RowIssue {
  part: RowPart;
  code: string;
  detail?: string;
  message?: string;
}

export type IssueMap = ReadonlyMap<string, readonly RowIssue[]>;

export interface ConditionNumbering {
  leaves: ReadonlyMap<string, number>;
  groups: ReadonlyMap<string, number>;
}
