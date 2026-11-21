import type { VariableEntry } from '../../components/VariablePicker/variables';
import type { CatalogueVariable } from './types';

export function toVariableEntries(items: readonly CatalogueVariable[]): VariableEntry[] {
  return items.map((item) => {
    const entry: VariableEntry = { path: item.path, insertText: item.insert_text, group: item.group };
    if (item.type !== null) {
      entry.type = item.type;
    }
    if (item.sample !== null) {
      entry.sample = item.sample;
    }
    if (item.description !== null) {
      entry.description = item.description;
    }
    return entry;
  });
}
