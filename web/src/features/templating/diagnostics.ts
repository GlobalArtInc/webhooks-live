import type { EditorDiagnostic } from '../editor/types';
import type { PreviewDiagnostic } from './types';

export type ProblemTarget = { kind: 'body' } | { kind: 'header'; index: number } | { kind: 'field'; field: string };

export interface ProblemItem {
  id: string;
  target: ProblemTarget;
  diagnostic: EditorDiagnostic;
}

export interface GroupedDiagnostics {
  body: EditorDiagnostic[];
  headers: Map<number, EditorDiagnostic[]>;
}

const FIELD_PREFIX = 'response.';
const BODY_FIELD = 'body';
const HEADER_VALUE_FIELD = /^headers\.(\d+)\.value$/;
const HEADER_NAME_FIELD = /^headers\.\d+\.name$/;

export const NO_PROBLEMS: readonly EditorDiagnostic[] = [];

export function normalizeField(field: string): string {
  return field.startsWith(FIELD_PREFIX) ? field.slice(FIELD_PREFIX.length) : field;
}

export function toEditorDiagnostic(diagnostic: PreviewDiagnostic): EditorDiagnostic {
  return {
    line: diagnostic.line,
    column: diagnostic.column,
    length: diagnostic.length,
    code: diagnostic.code,
    message: diagnostic.message,
    severity: diagnostic.severity,
  };
}

export function targetOf(field: string): ProblemTarget {
  const normalized = normalizeField(field);
  if (normalized === BODY_FIELD || normalized.startsWith(`${BODY_FIELD}.`)) {
    return { kind: 'body' };
  }
  const header = HEADER_VALUE_FIELD.exec(normalized);
  if (header !== null && header[1] !== undefined) {
    return { kind: 'header', index: Number(header[1]) };
  }
  return { kind: 'field', field: normalized };
}

export function problemItems(diagnostics: readonly PreviewDiagnostic[]): ProblemItem[] {
  return diagnostics
    .filter((diagnostic) => !HEADER_NAME_FIELD.test(normalizeField(diagnostic.field)))
    .map((diagnostic, position) => ({
      id: `${position}:${diagnostic.field}:${diagnostic.line}:${diagnostic.column}:${diagnostic.code}`,
      target: targetOf(diagnostic.field),
      diagnostic: toEditorDiagnostic(diagnostic),
    }));
}

export function groupDiagnostics(items: readonly ProblemItem[]): GroupedDiagnostics {
  const grouped: GroupedDiagnostics = { body: [], headers: new Map() };
  for (const item of items) {
    if (item.target.kind === 'body') {
      grouped.body.push(item.diagnostic);
    } else if (item.target.kind === 'header') {
      const existing = grouped.headers.get(item.target.index) ?? [];
      grouped.headers.set(item.target.index, [...existing, item.diagnostic]);
    }
  }
  return grouped;
}

export function countBySeverity(items: readonly ProblemItem[], severity: 'error' | 'warning'): number {
  return items.filter((item) => (item.diagnostic.severity ?? 'error') === severity).length;
}

export function sortProblems(items: readonly ProblemItem[]): ProblemItem[] {
  const rank = (item: ProblemItem): number => (item.target.kind === 'body' ? 0 : item.target.kind === 'header' ? 1 : 2);
  return [...items].sort((left, right) => {
    const byKind = rank(left) - rank(right);
    if (byKind !== 0) {
      return byKind;
    }
    const leftIndex = left.target.kind === 'header' ? left.target.index : 0;
    const rightIndex = right.target.kind === 'header' ? right.target.index : 0;
    return (
      leftIndex - rightIndex ||
      left.diagnostic.line - right.diagnostic.line ||
      left.diagnostic.column - right.diagnostic.column
    );
  });
}
