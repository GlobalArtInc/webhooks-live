import { describe, expect, it } from 'vitest';
import { isCidr } from './cidr';
import { mapFieldErrors } from './fieldErrors';
import { CONDITION_CATALOGUE, sampleSource } from './fixtures';
import { newLeaf, parseSource, serializeConditions } from './model';
import type { ConditionsDraft, LeafDraft } from './types';
import { validateConditions, validateLeaf } from './validation';

function leaf(patch: Partial<LeafDraft>): LeafDraft {
  return { ...newLeaf(), ...patch };
}

function codes(candidate: LeafDraft): string[] {
  return validateLeaf(candidate, CONDITION_CATALOGUE, ['http']).map((issue) => issue.code);
}

function sampleDraft(): ConditionsDraft {
  const parsed = parseSource(sampleSource(), CONDITION_CATALOGUE);
  if (!parsed.ok) {
    throw new Error('sample must parse');
  }
  return parsed.value;
}

describe('row validation', () => {
  it('marks an empty row incomplete and accepts a finished one', () => {
    expect(codes(newLeaf())).toEqual(['subject_required', 'arg_required']);
    expect(codes(leaf({ subject: 'method', op: 'eq', argText: 'POST' }))).toEqual([]);
    expect(codes(leaf({ subject: 'path', op: 'exists' }))).toEqual([]);
  });

  it('requires and checks the parameter of subjects that take one', () => {
    expect(codes(leaf({ subject: 'header', op: 'exists' }))).toEqual(['parameter_required']);
    expect(codes(leaf({ subject: 'header', parameter: 'x bad', op: 'exists' }))).toEqual(['parameter_invalid']);
    expect(codes(leaf({ subject: 'header', parameter: 'X-Id', op: 'exists' }))).toEqual([]);
    expect(codes(leaf({ subject: 'variable', parameter: '1abc', op: 'exists' }))).toEqual(['parameter_invalid']);
  });

  it('checks the operator against the subject and the argument against the operator', () => {
    expect(codes(leaf({ subject: 'method', op: 'gt', argText: '1' }))).toEqual(['operator_not_allowed']);
    expect(codes(leaf({ subject: 'amount', op: 'gt', argText: 'many' }))).toEqual(['arg_number']);
    expect(codes(leaf({ subject: 'amount', op: 'gt', argText: ' 12.5 ' }))).toEqual([]);
    expect(codes(leaf({ subject: 'method', op: 'in', argList: [] }))).toEqual(['arg_required']);
    expect(codes(leaf({ subject: 'sender', op: 'in_cidr', argList: ['10.0.0.0/8', '10.0.0.0/40'] }))).toEqual([
      'arg_cidr',
    ]);
    expect(codes(leaf({ subject: 'method', op: 'teleport', argText: 'x' }))).toEqual(['operator_unknown']);
  });

  it('flags a subject that does not exist for any selected kind of capture', () => {
    const issues = validateLeaf(leaf({ subject: 'dns_name', op: 'exists' }), CONDITION_CATALOGUE, ['http', 'email']);

    expect(issues.map((issue) => issue.code)).toEqual(['subject_trigger']);
  });

  it('counts a missing trigger kind together with the row problems', () => {
    const draft = sampleDraft();

    expect(validateConditions(draft, CONDITION_CATALOGUE).count).toBe(0);
    expect(validateConditions({ ...draft, triggers: [] }, CONDITION_CATALOGUE)).toMatchObject({
      triggersMissing: true,
      count: 1,
    });
  });
});

describe('address ranges', () => {
  it('accepts IPv4 and IPv6 ranges and rejects out-of-range parts', () => {
    expect(['203.0.113.0/24', '10.1.2.3', '2001:db8::/32', '::1'].map(isCidr)).toEqual([true, true, true, true]);
    expect(['300.1.1.1/8', '10.0.0.0/33', '2001:db8::/129', 'nonsense', '1:2:3:4:5:6:7:8:9'].map(isCidr)).toEqual([
      false,
      false,
      false,
      false,
      false,
    ]);
  });
});

describe('server field errors', () => {
  it('lands an error on the row, and on the control its path names', () => {
    const draft = sampleDraft();
    const { paths } = serializeConditions(draft, CONDITION_CATALOGUE);
    const [method, header] = draft.root.children;

    const mapped = mapFieldErrors(
      [
        { field: 'condition.all.0.arg', code: 'regex_invalid', message: 'bad pattern' },
        { field: 'condition.all.1.value', code: 'invalid_format', message: 'bad header name' },
        { field: 'condition.all.2.any.1.not.arg.0', code: 'invalid_format', message: 'bad range' },
      ],
      paths,
      draft.root,
      CONDITION_CATALOGUE,
    );

    expect(mapped.rows.get(method?.id ?? '')?.map((issue) => issue.part)).toEqual(['arg']);
    expect(mapped.rows.get(header?.id ?? '')?.map((issue) => issue.part)).toEqual(['parameter']);
    const [, , nested] = draft.root.children;
    const negated = nested?.kind === 'group' ? nested.children[1] : undefined;
    expect(mapped.rows.get(negated?.id ?? '')?.[0]).toMatchObject({ part: 'arg', message: 'bad range' });
    expect(mapped.general).toEqual([]);
  });

  it('routes trigger, on-match and unknown paths away from the rows instead of dropping them', () => {
    const draft = sampleDraft();
    const { paths } = serializeConditions(draft, CONDITION_CATALOGUE);

    const mapped = mapFieldErrors(
      [
        { field: 'triggers.0', code: 'invalid_format', message: 'unknown kind' },
        { field: 'on_match', code: 'invalid_format', message: 'unknown mode' },
        { field: 'name', code: 'too_long', message: 'too long' },
        { field: 'condition.all', code: 'too_many', message: 'too many conditions' },
      ],
      paths,
      draft.root,
      CONDITION_CATALOGUE,
    );

    expect(mapped.triggers).toHaveLength(1);
    expect(mapped.onMatch).toHaveLength(1);
    expect(mapped.general.map((error) => error.field)).toEqual(['name']);
    expect(mapped.rows.get(draft.root.id)?.[0]?.message).toBe('too many conditions');
  });
});
