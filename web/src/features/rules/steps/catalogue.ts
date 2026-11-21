import type { StepCatalogueEntry, StepClass } from './types';

export const CLASS_ORDER: readonly StepClass[] = ['response', 'compute', 'state', 'fetch', 'effect'];

export interface EntryGroup {
  id: StepClass;
  entries: StepCatalogueEntry[];
}

export function filterEntries(catalogue: readonly StepCatalogueEntry[], query: string): StepCatalogueEntry[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') {
    return [...catalogue];
  }
  return catalogue.filter(
    (entry) =>
      entry.title.toLowerCase().includes(needle) ||
      entry.kind.toLowerCase().includes(needle) ||
      entry.description.toLowerCase().includes(needle),
  );
}

export function groupEntries(entries: readonly StepCatalogueEntry[]): EntryGroup[] {
  const groups: EntryGroup[] = [];
  for (const id of CLASS_ORDER) {
    const members = entries.filter((entry) => entry.class === id);
    if (members.length > 0) {
      groups.push({ id, entries: members });
    }
  }
  return groups;
}
