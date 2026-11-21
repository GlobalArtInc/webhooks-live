import { describe, expect, it } from 'vitest';
import { matchSubject } from './catalogue';
import { CONDITION_CATALOGUE, sampleSource } from './fixtures';
import {
  addChild,
  changedNodeIds,
  conditionsKey,
  countLeaves,
  depthOf,
  fitsLimits,
  newGroup,
  newLeaf,
  numberNodes,
  parseCondition,
  parseSource,
  removeNode,
  serializeConditions,
  updateGroup,
  updateLeaf,
} from './model';
import type { ConditionsDraft, GroupDraft } from './types';

function draftOf(condition: unknown): ConditionsDraft {
  const parsed = parseSource({ ...sampleSource(), condition }, CONDITION_CATALOGUE);
  if (!parsed.ok) {
    throw new Error(`unexpected parse failure: ${parsed.reason}`);
  }
  return parsed.value;
}

function nest(levels: number): GroupDraft {
  let group: GroupDraft = newGroup();
  for (let level = 1; level < levels; level += 1) {
    group = { ...newGroup(), children: [group] };
  }
  return group;
}

describe('condition parsing and serialisation', () => {
  it('round-trips a nested condition with not, lists and a numeric argument', () => {
    const condition = {
      all: [
        { value: 'request.method', op: 'eq', arg: 'POST' },
        { value: 'request.header.x-event-type', op: 'exists' },
        { value: 'request.json.amount', op: 'gt', arg: 100 },
        { any: [{ not: { value: 'request.client_ip', op: 'in_cidr', arg: ['10.0.0.0/8'] } }, {}] },
      ],
    };

    const { input } = serializeConditions(draftOf(condition), CONDITION_CATALOGUE);

    expect(input.condition).toEqual(condition);
  });

  it('wraps a root leaf or not in an all group and treats an empty object as always', () => {
    const leaf = { value: 'request.method', op: 'eq', arg: 'GET' };

    expect(serializeConditions(draftOf(leaf), CONDITION_CATALOGUE).input.condition).toEqual({ all: [leaf] });
    expect(serializeConditions(draftOf({ not: leaf }), CONDITION_CATALOGUE).input.condition).toEqual({
      all: [{ not: leaf }],
    });
    expect(serializeConditions(draftOf({}), CONDITION_CATALOGUE).input.condition).toEqual({});
  });

  it('refuses shapes it would lose on save instead of dropping them', () => {
    const catalogue = CONDITION_CATALOGUE;

    expect(parseCondition({ all: [], any: [] }, catalogue).ok).toBe(false);
    expect(parseCondition({ value: 'request.method', op: 'eq', arg: 'GET', extra: 1 }, catalogue).ok).toBe(false);
    expect(parseCondition({ all: 'nope' }, catalogue).ok).toBe(false);
    expect(parseCondition({ value: 'request.method', op: 'in', arg: [1] }, catalogue).ok).toBe(false);
    expect(parseSource({ ...sampleSource(), triggers: ['fax'] }, catalogue).ok).toBe(false);
    expect(parseSource({ ...sampleSource(), on_match: 'maybe' }, catalogue).ok).toBe(false);
  });

  it('picks the subject by exact path first and then by the longest prefix, with a variable fallback', () => {
    expect(matchSubject(CONDITION_CATALOGUE, 'request.json.amount').subject.id).toBe('amount');
    expect(matchSubject(CONDITION_CATALOGUE, 'request.json.data.id')).toMatchObject({
      subject: { id: 'json' },
      parameter: 'data.id',
    });
    expect(matchSubject(CONDITION_CATALOGUE, 'steps.crm.status')).toMatchObject({
      subject: { id: 'variable' },
      parameter: 'steps.crm.status',
    });
  });

  it('normalises header names and JSON paths when writing the variable path', () => {
    const draft = draftOf({ all: [{ value: 'request.header.x-id', op: 'exists' }] });
    const row = draft.root.children[0];
    if (row === undefined || row.kind !== 'leaf') {
      throw new Error('expected a leaf');
    }
    const edited = updateLeaf(draft.root, row.id, { subject: 'header', parameter: ' X-Event-Type ' });
    const json = updateLeaf(edited, row.id, { subject: 'json', parameter: '$.data.id' });

    expect(serializeConditions({ ...draft, root: edited }, CONDITION_CATALOGUE).input.condition).toEqual({
      all: [{ value: 'request.header.x-event-type', op: 'exists' }],
    });
    expect(serializeConditions({ ...draft, root: json }, CONDITION_CATALOGUE).input.condition).toEqual({
      all: [{ value: 'request.json.data.id', op: 'exists' }],
    });
  });

  it('records the API path of a negated node both for the wrapper and the node, so server errors can find it', () => {
    const draft = draftOf({
      all: [{ value: 'request.method', op: 'eq', arg: 'GET' }, { not: { value: 'x', op: 'eq', arg: '1' } }],
    });
    const negated = draft.root.children[1];

    const paths = serializeConditions(draft, CONDITION_CATALOGUE).paths.filter((entry) => entry.id === negated?.id);

    expect(paths.map((entry) => entry.path).sort()).toEqual(['condition.all.1', 'condition.all.1.not']);
  });

  it('compares drafts by their serialised form, ignoring row ids', () => {
    const first = draftOf(sampleSource().condition);
    const second = draftOf(sampleSource().condition);

    expect(first.root.id).not.toBe(second.root.id);
    expect(conditionsKey(first, CONDITION_CATALOGUE)).toBe(conditionsKey(second, CONDITION_CATALOGUE));
  });
});

describe('tree operations and limits', () => {
  it('adds, updates and removes nodes without touching untouched branches', () => {
    const draft = draftOf(sampleSource().condition);
    const [first, , nested] = draft.root.children;
    const leaf = newLeaf();

    const added = addChild(draft.root, draft.root.id, leaf);
    const changed = updateGroup(added, draft.root.id, { mode: 'any' });
    const removed = removeNode(changed, leaf.id);

    expect(added.children).toHaveLength(4);
    expect(changed.mode).toBe('any');
    expect(removed.children).toHaveLength(3);
    expect(removed.children[0]).toBe(first);
    expect(removed.children[2]).toBe(nested);
    expect(removeNode(draft.root, draft.root.id)).toBe(draft.root);
  });

  it('numbers conditions in reading order and nested groups from one', () => {
    const numbering = numberNodes(draftOf(sampleSource().condition).root);

    expect([...numbering.leaves.values()]).toEqual([1, 2, 3, 4]);
    expect([...numbering.groups.values()]).toEqual([1]);
  });

  it('counts a not as a level and refuses trees above the depth and leaf limits', () => {
    const limits = { maxDepth: 4, maxLeaves: 3 };

    expect(depthOf(nest(4))).toBe(4);
    expect(fitsLimits(nest(4), limits)).toBe(true);
    expect(fitsLimits(nest(5), limits)).toBe(false);

    const deep = nest(4);
    const inner = deep.children[0];
    const negatedInner = inner === undefined ? deep : updateGroup(deep, inner.id, { negate: true });
    expect(fitsLimits(negatedInner, limits)).toBe(false);

    const full = [newLeaf(), newLeaf(), newLeaf()].reduce((root, leaf) => addChild(root, root.id, leaf), newGroup());
    expect(countLeaves(full)).toBe(3);
    expect(fitsLimits(addChild(full, full.id, newLeaf()), limits)).toBe(false);
  });

  it('reports which nodes a change replaced', () => {
    const draft = draftOf(sampleSource().condition);
    const target = draft.root.children[0];
    if (target === undefined) {
      throw new Error('expected a row');
    }

    const next = updateLeaf(draft.root, target.id, { argText: 'PUT' });

    expect(changedNodeIds(draft.root, next).sort()).toEqual([draft.root.id, target.id].sort());
  });
});
