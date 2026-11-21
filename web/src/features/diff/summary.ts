import type { Capture, NameValue } from '../../api/types';
import { isVolatileHeader, normaliseCapture } from './normalize';
import type {
  BodyComparison,
  FieldEquality,
  HeaderCounts,
  NormaliseOptions,
  NormalisedCapture,
  StructuralSummary,
} from './types';

function groupByName(rows: NameValue[]): Map<string, string[]> {
  const groups = new Map<string, string[]>();
  for (const row of rows) {
    const key = row.name.toLowerCase();
    groups.set(key, [...(groups.get(key) ?? []), row.value]);
  }
  return groups;
}

function sameValues(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function countHeaders(left: NameValue[], right: NameValue[]): Omit<HeaderCounts, 'hidden'> {
  const leftGroups = groupByName(left);
  const rightGroups = groupByName(right);
  const counts = { added: 0, removed: 0, changed: 0, same: 0 };
  for (const [name, leftValues] of leftGroups) {
    const rightValues = rightGroups.get(name);
    if (rightValues === undefined) {
      counts.removed += 1;
    } else if (sameValues(leftValues, rightValues)) {
      counts.same += 1;
    } else {
      counts.changed += 1;
    }
  }
  for (const name of rightGroups.keys()) {
    if (!leftGroups.has(name)) {
      counts.added += 1;
    }
  }
  return counts;
}

function countHidden(left: Capture, right: Capture, options: NormaliseOptions): number {
  if (!options.hideVolatile) {
    return 0;
  }
  const names = new Set([...left.headers, ...right.headers].map((header) => header.name.toLowerCase()));
  return [...names].filter((name) => isVolatileHeader(name)).length;
}

function fieldEquality(left: NormalisedCapture, right: NormalisedCapture): FieldEquality {
  return {
    method: left.summary.method === right.summary.method,
    path: left.summary.path === right.summary.path,
    query: left.summary.query === right.summary.query,
  };
}

function bodyComparison(
  left: Capture,
  right: Capture,
  leftNormalised: NormalisedCapture,
  rightNormalised: NormalisedCapture,
): BodyComparison {
  if (left.body.encoding === 'base64' || right.body.encoding === 'base64') {
    const bothBinary = left.body.encoding === 'base64' && right.body.encoding === 'base64';
    return bothBinary && left.body.data === right.body.data ? 'same' : 'different';
  }
  return leftNormalised.body === rightNormalised.body ? 'same' : 'different';
}

export function compareCaptures(
  left: Capture,
  right: Capture,
  options: NormaliseOptions,
  compareBodies: boolean,
): StructuralSummary {
  const digestFree: NormaliseOptions = { ...options, binaryDigest: null };
  const leftNormalised = normaliseCapture(left, digestFree);
  const rightNormalised = normaliseCapture(right, digestFree);
  const counts = countHeaders(leftNormalised.headerRows, rightNormalised.headerRows);
  return {
    fields: fieldEquality(leftNormalised, rightNormalised),
    headers: { ...counts, hidden: countHidden(left, right, options) },
    body: compareBodies ? bodyComparison(left, right, leftNormalised, rightNormalised) : 'not-compared',
  };
}

export function isIdentical(summary: StructuralSummary): boolean {
  const { fields, headers, body } = summary;
  return (
    fields.method &&
    fields.path &&
    fields.query &&
    headers.added === 0 &&
    headers.removed === 0 &&
    headers.changed === 0 &&
    body === 'same'
  );
}
