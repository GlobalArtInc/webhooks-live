import { findOperator, findSubject, normalizeParameter, resolveArgKind, subjectAvailable } from './catalogue';
import { isCidr } from './cidr';
import { walkNodes } from './model';
import type { ConditionCatalogue, ConditionsDraft, IssueMap, LeafDraft, ParameterKind, RowIssue } from './types';

export interface ConditionsValidation {
  rows: IssueMap;
  triggersMissing: boolean;
  count: number;
}

const HEADER_NAME = /^[A-Za-z0-9!#$%&'*+.^_`|~-]+$/;
const VARIABLE_PATH = /^[A-Za-z_][A-Za-z0-9_.-]*$/;
const NAME = /^[^\s.]+$/;

function parameterProblem(kind: ParameterKind, text: string): boolean {
  switch (kind) {
    case 'header_name':
      return !HEADER_NAME.test(text);
    case 'variable':
      return !VARIABLE_PATH.test(text);
    case 'name':
      return !NAME.test(text);
    case 'json_path':
      return /\s/.test(text);
  }
}

export function validateLeaf(
  leaf: LeafDraft,
  catalogue: ConditionCatalogue,
  triggers: ConditionsDraft['triggers'],
): RowIssue[] {
  const issues: RowIssue[] = [];
  const subject = leaf.subject === '' ? undefined : findSubject(catalogue, leaf.subject);
  if (leaf.subject === '') {
    issues.push({ part: 'subject', code: 'subject_required' });
  } else if (subject === undefined) {
    issues.push({ part: 'subject', code: 'subject_unknown', detail: leaf.subject });
  } else {
    if (triggers.length > 0 && !subjectAvailable(subject, triggers)) {
      issues.push({ part: 'subject', code: 'subject_trigger' });
    }
    if (subject.parameter !== null) {
      const text = normalizeParameter(subject.parameter.kind, leaf.parameter);
      if (text === '') {
        issues.push({ part: 'parameter', code: 'parameter_required' });
      } else if (parameterProblem(subject.parameter.kind, text)) {
        issues.push({ part: 'parameter', code: 'parameter_invalid' });
      }
    }
  }
  const operator = findOperator(catalogue, leaf.op);
  if (leaf.op === '') {
    issues.push({ part: 'operator', code: 'operator_required' });
  } else if (operator === undefined) {
    issues.push({ part: 'operator', code: 'operator_unknown', detail: leaf.op });
  } else if (subject !== undefined && !operator.value_types.includes(subject.value_type)) {
    issues.push({ part: 'operator', code: 'operator_not_allowed' });
  }
  if (operator !== undefined) {
    issues.push(...argumentIssues(leaf, resolveArgKind(operator, subject)));
  }
  return issues;
}

function argumentIssues(leaf: LeafDraft, kind: ReturnType<typeof resolveArgKind>): RowIssue[] {
  switch (kind) {
    case 'none':
      return [];
    case 'list':
      return leaf.argList.length === 0 ? [{ part: 'arg', code: 'arg_required' }] : [];
    case 'cidr_list': {
      if (leaf.argList.length === 0) {
        return [{ part: 'arg', code: 'arg_required' }];
      }
      const bad = leaf.argList.find((entry) => !isCidr(entry));
      return bad === undefined ? [] : [{ part: 'arg', code: 'arg_cidr', detail: bad }];
    }
    case 'number': {
      const text = leaf.argText.trim();
      if (text === '') {
        return [{ part: 'arg', code: 'arg_required' }];
      }
      return Number.isFinite(Number(text)) ? [] : [{ part: 'arg', code: 'arg_number' }];
    }
    default:
      return leaf.argText === '' ? [{ part: 'arg', code: 'arg_required' }] : [];
  }
}

export function validateConditions(draft: ConditionsDraft, catalogue: ConditionCatalogue): ConditionsValidation {
  const rows = new Map<string, RowIssue[]>();
  let count = 0;
  for (const node of walkNodes(draft.root)) {
    if (node.kind === 'leaf') {
      const issues = validateLeaf(node, catalogue, draft.triggers);
      if (issues.length > 0) {
        rows.set(node.id, issues);
        count += issues.length;
      }
    }
  }
  const triggersMissing = draft.triggers.length === 0;
  return { rows, triggersMissing, count: count + (triggersMissing ? 1 : 0) };
}
