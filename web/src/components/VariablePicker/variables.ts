import type { CompletionEntry, CompletionSource } from '../../features/editor/types';

export interface VariableEntry {
  path: string;
  insertText: string;
  group: string;
  type?: string;
  sample?: string;
  description?: string;
}

export interface VariableGroup {
  id: string;
  entries: VariableEntry[];
}

export function filterVariables(entries: readonly VariableEntry[], query: string): VariableEntry[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') {
    return [...entries];
  }
  return entries.filter(
    (entry) => entry.path.toLowerCase().includes(needle) || (entry.description ?? '').toLowerCase().includes(needle),
  );
}

export function groupVariables(entries: readonly VariableEntry[]): VariableGroup[] {
  const groups: VariableGroup[] = [];
  for (const entry of entries) {
    const existing = groups.find((group) => group.id === entry.group);
    if (existing === undefined) {
      groups.push({ id: entry.group, entries: [entry] });
    } else {
      existing.entries.push(entry);
    }
  }
  return groups;
}

function detailOf(entry: VariableEntry): string | undefined {
  const parts = [entry.type, entry.sample].filter((part): part is string => part !== undefined && part !== '');
  return parts.length === 0 ? undefined : parts.join('  ');
}

export function variableCompletionSource(entries: readonly VariableEntry[]): CompletionSource {
  return (query) =>
    entries.map((entry): CompletionEntry => {
      const completion: CompletionEntry = {
        label: entry.path,
        insertText: query.insideTemplate ? entry.path : entry.insertText,
        kind: 'variable',
      };
      const detail = detailOf(entry);
      if (detail !== undefined) {
        completion.detail = detail;
      }
      if (entry.description !== undefined) {
        completion.description = entry.description;
      }
      return completion;
    });
}
