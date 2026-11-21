import { VARIABLE_SUBJECT_ID } from './constants';
import type {
  ConditionCatalogue,
  LeafDraft,
  OperatorArgKind,
  OperatorEntry,
  ParameterKind,
  SubjectEntry,
  TriggerKind,
} from './types';

export const VARIABLE_SUBJECT: SubjectEntry = {
  id: VARIABLE_SUBJECT_ID,
  label: 'Variable',
  prefix: '',
  kinds: ['http', 'email', 'dns'],
  value_type: 'string',
  parameter: { kind: 'variable' },
};

export type ResolvedArgKind = Exclude<OperatorArgKind, 'value'> | 'text';

export interface SubjectMatch {
  subject: SubjectEntry;
  parameter: string;
}

export function subjectsOf(catalogue: ConditionCatalogue): readonly SubjectEntry[] {
  return catalogue.subjects.some((subject) => subject.prefix === '')
    ? catalogue.subjects
    : [...catalogue.subjects, VARIABLE_SUBJECT];
}

export function findSubject(catalogue: ConditionCatalogue, id: string): SubjectEntry | undefined {
  return subjectsOf(catalogue).find((subject) => subject.id === id);
}

export function findOperator(catalogue: ConditionCatalogue, id: string): OperatorEntry | undefined {
  return catalogue.operators.find((operator) => operator.id === id);
}

export function matchSubject(catalogue: ConditionCatalogue, path: string): SubjectMatch {
  const subjects = subjectsOf(catalogue);
  const exact = subjects.find((subject) => subject.parameter === null && subject.prefix === path);
  if (exact !== undefined) {
    return { subject: exact, parameter: '' };
  }
  let best: SubjectEntry | undefined;
  for (const subject of subjects) {
    const fits = subject.parameter !== null && path.startsWith(subject.prefix) && path.length > subject.prefix.length;
    if (fits && (best === undefined || subject.prefix.length > best.prefix.length)) {
      best = subject;
    }
  }
  const chosen = best ?? subjects.find((subject) => subject.prefix === '') ?? VARIABLE_SUBJECT;
  return { subject: chosen, parameter: path.slice(chosen.prefix.length) };
}

export function operatorsFor(
  catalogue: ConditionCatalogue,
  subject: SubjectEntry | undefined,
): readonly OperatorEntry[] {
  if (subject === undefined) {
    return catalogue.operators;
  }
  return catalogue.operators.filter((operator) => operator.value_types.includes(subject.value_type));
}

export function resolveArgKind(
  operator: OperatorEntry | undefined,
  subject: SubjectEntry | undefined,
): ResolvedArgKind {
  if (operator === undefined) {
    return 'text';
  }
  if (operator.arg === 'value') {
    return subject?.value_type === 'number' ? 'number' : 'text';
  }
  return operator.arg;
}

export function normalizeParameter(kind: ParameterKind | undefined, text: string): string {
  const trimmed = text.trim();
  switch (kind) {
    case 'header_name':
      return trimmed.toLowerCase();
    case 'json_path':
      return trimmed.replace(/^\$\.?/, '');
    default:
      return trimmed;
  }
}

export function leafPath(catalogue: ConditionCatalogue, leaf: LeafDraft): string {
  const subject = findSubject(catalogue, leaf.subject);
  if (subject === undefined) {
    return '';
  }
  return subject.parameter === null
    ? subject.prefix
    : `${subject.prefix}${normalizeParameter(subject.parameter.kind, leaf.parameter)}`;
}

export function subjectAvailable(subject: SubjectEntry, triggers: readonly TriggerKind[]): boolean {
  return subject.kinds.some((kind) => triggers.includes(kind));
}
