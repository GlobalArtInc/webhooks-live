export type ChipValidator = (value: string) => string | null;

export type ChipNormalizer = (value: string) => string;

export interface ChipRules {
  maxItems?: number;
  maxLength?: number;
  allowDuplicates: boolean;
  validate?: ChipValidator;
  normalize?: ChipNormalizer;
}

export type ChipFailureKind = 'duplicate' | 'limit' | 'tooLong' | 'invalid';

export interface ChipFailure {
  kind: ChipFailureKind;
  value: string;
  detail: string;
}

export interface ChipBatch {
  accepted: string[];
  failure: ChipFailure | null;
  rest: string[];
}

const SEPARATORS = /[,\n\r]+/;

export function splitChips(text: string): string[] {
  return text
    .split(SEPARATORS)
    .map((part) => part.trim())
    .filter((part) => part !== '');
}

export function checkChip(existing: readonly string[], raw: string, rules: ChipRules): ChipFailure | null {
  const value = rules.normalize === undefined ? raw : rules.normalize(raw);
  if (rules.maxItems !== undefined && existing.length >= rules.maxItems) {
    return { kind: 'limit', value, detail: String(rules.maxItems) };
  }
  if (rules.maxLength !== undefined && value.length > rules.maxLength) {
    return { kind: 'tooLong', value, detail: String(rules.maxLength) };
  }
  if (!rules.allowDuplicates && existing.includes(value)) {
    return { kind: 'duplicate', value, detail: '' };
  }
  const message = rules.validate === undefined ? null : rules.validate(value);
  return message === null ? null : { kind: 'invalid', value, detail: message };
}

export function addChips(existing: readonly string[], pieces: readonly string[], rules: ChipRules): ChipBatch {
  const accepted: string[] = [];
  for (let index = 0; index < pieces.length; index += 1) {
    const piece = pieces[index] ?? '';
    const failure = checkChip([...existing, ...accepted], piece, rules);
    if (failure !== null) {
      return { accepted, failure, rest: pieces.slice(index) };
    }
    accepted.push(rules.normalize === undefined ? piece : rules.normalize(piece));
  }
  return { accepted, failure: null, rest: [] };
}
