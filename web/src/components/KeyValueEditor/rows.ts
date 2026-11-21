export interface KeyValueRow {
  id: string;
  name: string;
  value: string;
}

export type KeyValueField = 'name' | 'value' | 'row';

export interface KeyValueRowError {
  rowId: string;
  field: KeyValueField;
  message: string;
}

export interface KeyValueLimits {
  maxNameLength?: number;
  maxValueLength?: number;
}

export interface LengthFailure {
  rowId: string;
  field: 'name' | 'value';
  max: number;
}

let rowCounter = 0;

export function createKeyValueRow(name = '', value = ''): KeyValueRow {
  rowCounter += 1;
  return { id: `kv-${rowCounter}`, name, value };
}

export function moveRow(rows: readonly KeyValueRow[], index: number, delta: number): KeyValueRow[] {
  const target = index + delta;
  if (index < 0 || index >= rows.length || target < 0 || target >= rows.length) {
    return [...rows];
  }
  const next = [...rows];
  const [moved] = next.splice(index, 1);
  if (moved !== undefined) {
    next.splice(target, 0, moved);
  }
  return next;
}

export function removeRow(rows: readonly KeyValueRow[], index: number): KeyValueRow[] {
  return rows.filter((_, position) => position !== index);
}

export function updateRow(
  rows: readonly KeyValueRow[],
  index: number,
  change: Partial<Omit<KeyValueRow, 'id'>>,
): KeyValueRow[] {
  return rows.map((row, position) => (position === index ? { ...row, ...change } : row));
}

export function findLengthFailures(rows: readonly KeyValueRow[], limits: KeyValueLimits): LengthFailure[] {
  const failures: LengthFailure[] = [];
  for (const row of rows) {
    if (limits.maxNameLength !== undefined && row.name.length > limits.maxNameLength) {
      failures.push({ rowId: row.id, field: 'name', max: limits.maxNameLength });
    }
    if (limits.maxValueLength !== undefined && row.value.length > limits.maxValueLength) {
      failures.push({ rowId: row.id, field: 'value', max: limits.maxValueLength });
    }
  }
  return failures;
}
