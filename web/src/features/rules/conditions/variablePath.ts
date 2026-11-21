import type { VariableEntry } from '../../../components/VariablePicker/variables';

const TEMPLATE_TOKEN = /^\{\{\s*([^{}\s|]+)\s*\}\}$/;

export function variablePathOf(entry: VariableEntry): string {
  const match = TEMPLATE_TOKEN.exec(entry.insertText.trim());
  return match?.[1] ?? entry.path;
}
