import type { ShapeIssue, RuleStep as StepDraft, StepCatalogueEntry } from '../steps/types';
import type { ConditionCatalogue, GroupDraft, RowIssue, RowPart } from '../conditions/types';
import type { ConditionsValidation } from '../conditions/validation';
import type { FieldError } from '../../../api/types';

export interface RuleDraft {
  name: string;
  root: GroupDraft;
  steps: StepDraft[];
}

export interface RuleCatalogue {
  conditions: ConditionCatalogue;
  steps: readonly StepCatalogueEntry[];
}

export type StepProblemKind = 'required' | 'unsupportedName' | 'shape';

export interface StepProblem {
  stepId: string;
  field: string | null;
  kind: StepProblemKind;
  shape: ShapeIssue | null;
}

export interface RuleValidation {
  nameMissing: boolean;
  nameTooLong: boolean;
  stepsMissing: boolean;
  conditions: ConditionsValidation;
  steps: StepProblem[];
  count: number;
}

export interface ServerStepError {
  stepId: string;
  field: string | null;
  message: string;
}

export interface ServerProblems {
  rows: ReadonlyMap<string, readonly RowIssue[]>;
  name: readonly FieldError[];
  steps: readonly ServerStepError[];
  general: readonly FieldError[];
}

export type FocusTarget =
  | { kind: 'name' }
  | { kind: 'row'; id: string; part: RowPart }
  | { kind: 'step'; stepId: string; field: string | null }
  | { kind: 'steps' };

export interface FocusRequest {
  target: FocusTarget;
  serial: number;
}

export interface StatusNote {
  kind: 'saved' | 'problems';
  count: number;
  serial: number;
}
