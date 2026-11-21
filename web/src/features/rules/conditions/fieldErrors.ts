import type { FieldError } from '../../../api/types';
import { findSubject } from './catalogue';
import { findNode, type NodePath } from './model';
import type { ConditionCatalogue, GroupDraft, RowIssue, RowPart } from './types';

export interface MappedFieldErrors {
  rows: Map<string, RowIssue[]>;
  triggers: FieldError[];
  onMatch: FieldError[];
  general: FieldError[];
}

function leafPart(rest: string, hasParameter: boolean): RowPart {
  const [head = ''] = rest.split('.');
  switch (head) {
    case 'value':
      return hasParameter ? 'parameter' : 'subject';
    case 'op':
      return 'operator';
    case 'arg':
      return 'arg';
    default:
      return 'row';
  }
}

function issueOf(field: FieldError, part: RowPart): RowIssue {
  return { part, code: field.code ?? 'invalid', message: field.message };
}

function startsAt(field: string, path: string): boolean {
  return field === path || field.startsWith(`${path}.`);
}

export function mapFieldErrors(
  fields: readonly FieldError[],
  paths: readonly NodePath[],
  root: GroupDraft,
  catalogue: ConditionCatalogue,
): MappedFieldErrors {
  const ordered = [...paths].sort((left, right) => right.path.length - left.path.length);
  const mapped: MappedFieldErrors = { rows: new Map(), triggers: [], onMatch: [], general: [] };
  for (const field of fields) {
    if (startsAt(field.field, 'triggers')) {
      mapped.triggers.push(field);
      continue;
    }
    if (startsAt(field.field, 'on_match')) {
      mapped.onMatch.push(field);
      continue;
    }
    const hit = ordered.find((entry) => startsAt(field.field, entry.path));
    if (hit === undefined) {
      mapped.general.push(field);
      continue;
    }
    const node = findNode(root, hit.id);
    let part: RowPart = 'row';
    if (node?.kind === 'leaf') {
      const subject = findSubject(catalogue, node.subject);
      const hasParameter = subject !== undefined && subject.parameter !== null;
      part = leafPart(field.field.slice(hit.path.length + 1), hasParameter);
    }
    mapped.rows.set(hit.id, [...(mapped.rows.get(hit.id) ?? []), issueOf(field, part)]);
  }
  return mapped;
}
