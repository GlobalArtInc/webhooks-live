import { findOperator, findSubject, leafPath, matchSubject, resolveArgKind } from './catalogue';
import { TRIGGER_KINDS } from './constants';
import type {
  ApiCondition,
  ConditionCatalogue,
  ConditionLimits,
  ConditionNumbering,
  ConditionsDraft,
  ConditionsInput,
  GroupDraft,
  LeafCondition,
  LeafDraft,
  NodeDraft,
  OnMatch,
  RuleConditionsSource,
  TriggerKind,
} from './types';

export interface NodePath {
  id: string;
  path: string;
}

export interface SerializedConditions {
  input: ConditionsInput;
  paths: NodePath[];
}

export type ParseResult<T> = { ok: true; value: T } | { ok: false; reason: string };

type UnknownRecord = Record<string, unknown>;

const LEAF_KEYS: readonly string[] = ['value', 'op', 'arg'];

let sequence = 0;

export function nextId(): string {
  sequence += 1;
  return `cond-${sequence}`;
}

export function newLeaf(): LeafDraft {
  return { kind: 'leaf', id: nextId(), negate: false, subject: '', parameter: '', op: 'eq', argText: '', argList: [] };
}

export function newGroup(): GroupDraft {
  return { kind: 'group', id: nextId(), negate: false, mode: 'all', children: [] };
}

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string');
}

function fail(reason: string): ParseResult<never> {
  return { ok: false, reason };
}

function parseLeaf(record: UnknownRecord, catalogue: ConditionCatalogue): ParseResult<NodeDraft> {
  const { value, op, arg } = record;
  if (typeof value !== 'string' || typeof op !== 'string') {
    return fail('leaf');
  }
  if (arg !== undefined && arg !== null && typeof arg !== 'string' && typeof arg !== 'number' && !isStringArray(arg)) {
    return fail('arg');
  }
  const { subject, parameter } = matchSubject(catalogue, value);
  const leaf: LeafDraft = {
    kind: 'leaf',
    id: nextId(),
    negate: false,
    subject: subject.id,
    parameter,
    op,
    argText: typeof arg === 'string' || typeof arg === 'number' ? String(arg) : '',
    argList: isStringArray(arg) ? [...arg] : typeof arg === 'string' ? [arg] : [],
  };
  return { ok: true, value: leaf };
}

function parseChildren(items: unknown, catalogue: ConditionCatalogue): ParseResult<NodeDraft[]> {
  if (!Array.isArray(items)) {
    return fail('children');
  }
  const children: NodeDraft[] = [];
  for (const item of items) {
    const parsed = parseNode(item, catalogue);
    if (!parsed.ok) {
      return parsed;
    }
    children.push(parsed.value);
  }
  return { ok: true, value: children };
}

function parseNode(value: unknown, catalogue: ConditionCatalogue): ParseResult<NodeDraft> {
  if (!isRecord(value)) {
    return fail('shape');
  }
  const keys = Object.keys(value);
  if (keys.length === 0) {
    return { ok: true, value: newGroup() };
  }
  if (keys.length === 1 && (keys[0] === 'all' || keys[0] === 'any')) {
    const mode = keys[0];
    const children = parseChildren(value[mode], catalogue);
    if (!children.ok) {
      return children;
    }
    return { ok: true, value: { ...newGroup(), mode, children: children.value } };
  }
  if (keys.length === 1 && keys[0] === 'not') {
    const inner = parseNode(value.not, catalogue);
    return inner.ok ? { ok: true, value: { ...inner.value, negate: !inner.value.negate } } : inner;
  }
  if (keys.every((key) => LEAF_KEYS.includes(key))) {
    return parseLeaf(value, catalogue);
  }
  return fail('keys');
}

export function parseCondition(value: unknown, catalogue: ConditionCatalogue): ParseResult<GroupDraft> {
  const parsed = parseNode(value, catalogue);
  if (!parsed.ok) {
    return parsed;
  }
  if (parsed.value.kind === 'group' && !parsed.value.negate) {
    return { ok: true, value: parsed.value };
  }
  return { ok: true, value: { ...newGroup(), children: [parsed.value] } };
}

function isTriggerKind(value: string): value is TriggerKind {
  return TRIGGER_KINDS.some((kind) => kind === value);
}

function isOnMatch(value: string): value is OnMatch {
  return value === 'next' || value === 'stop';
}

export function orderTriggers(triggers: readonly TriggerKind[]): TriggerKind[] {
  return TRIGGER_KINDS.filter((kind) => triggers.includes(kind));
}

export function parseSource(source: RuleConditionsSource, catalogue: ConditionCatalogue): ParseResult<ConditionsDraft> {
  const triggers: TriggerKind[] = [];
  for (const trigger of source.triggers) {
    if (!isTriggerKind(trigger)) {
      return fail('trigger');
    }
    triggers.push(trigger);
  }
  if (!isOnMatch(source.on_match)) {
    return fail('on_match');
  }
  const root = parseCondition(source.condition, catalogue);
  if (!root.ok) {
    return root;
  }
  return { ok: true, value: { triggers: orderTriggers(triggers), root: root.value, onMatch: source.on_match } };
}

function serializeLeaf(leaf: LeafDraft, catalogue: ConditionCatalogue): LeafCondition {
  const subject = findSubject(catalogue, leaf.subject);
  const kind = resolveArgKind(findOperator(catalogue, leaf.op), subject);
  const condition: LeafCondition = { value: leafPath(catalogue, leaf), op: leaf.op };
  switch (kind) {
    case 'none':
      break;
    case 'number': {
      const parsed = Number(leaf.argText.trim());
      condition.arg = leaf.argText.trim() !== '' && Number.isFinite(parsed) ? parsed : leaf.argText;
      break;
    }
    case 'list':
    case 'cidr_list':
      condition.arg = [...leaf.argList];
      break;
    default:
      condition.arg = leaf.argText;
  }
  return condition;
}

function serializeNode(
  node: NodeDraft,
  path: string,
  catalogue: ConditionCatalogue,
  paths: NodePath[],
  isRoot: boolean,
): ApiCondition {
  const wrapped = node.negate && !isRoot;
  const inner = wrapped ? `${path}.not` : path;
  if (wrapped) {
    paths.push({ id: node.id, path });
  }
  paths.push({ id: node.id, path: inner });
  let body: ApiCondition;
  if (node.kind === 'leaf') {
    body = serializeLeaf(node, catalogue);
  } else if (node.children.length === 0) {
    body = {};
  } else {
    const items = node.children.map((child, index) =>
      serializeNode(child, `${inner}.${node.mode}.${index}`, catalogue, paths, false),
    );
    body = node.mode === 'all' ? { all: items } : { any: items };
  }
  return wrapped ? { not: body } : body;
}

export function serializeConditions(draft: ConditionsDraft, catalogue: ConditionCatalogue): SerializedConditions {
  const paths: NodePath[] = [];
  const condition = serializeNode(draft.root, 'condition', catalogue, paths, true);
  return { input: { condition, triggers: orderTriggers(draft.triggers), on_match: draft.onMatch }, paths };
}

export function conditionsKey(draft: ConditionsDraft, catalogue: ConditionCatalogue): string {
  return JSON.stringify(serializeConditions(draft, catalogue).input);
}

function transform(node: NodeDraft, id: string, change: (node: NodeDraft) => NodeDraft | null): NodeDraft | null {
  if (node.id === id) {
    return change(node);
  }
  if (node.kind === 'leaf') {
    return node;
  }
  let changed = false;
  const children: NodeDraft[] = [];
  for (const child of node.children) {
    const next = transform(child, id, change);
    if (next !== child) {
      changed = true;
    }
    if (next !== null) {
      children.push(next);
    }
  }
  return changed ? { ...node, children } : node;
}

function apply(root: GroupDraft, id: string, change: (node: NodeDraft) => NodeDraft | null): GroupDraft {
  const next = transform(root, id, change);
  return next !== null && next.kind === 'group' ? next : root;
}

export type LeafPatch = Partial<Omit<LeafDraft, 'kind' | 'id'>>;

export type GroupPatch = Partial<Omit<GroupDraft, 'kind' | 'id' | 'children'>>;

export function updateLeaf(root: GroupDraft, id: string, patch: LeafPatch): GroupDraft {
  return apply(root, id, (node) => (node.kind === 'leaf' ? { ...node, ...patch } : node));
}

export function updateGroup(root: GroupDraft, id: string, patch: GroupPatch): GroupDraft {
  return apply(root, id, (node) => (node.kind === 'group' ? { ...node, ...patch } : node));
}

export function addChild(root: GroupDraft, groupId: string, child: NodeDraft): GroupDraft {
  return apply(root, groupId, (node) =>
    node.kind === 'group' ? { ...node, children: [...node.children, child] } : node,
  );
}

export function removeNode(root: GroupDraft, id: string): GroupDraft {
  return id === root.id ? root : apply(root, id, () => null);
}

export function findNode(node: NodeDraft, id: string): NodeDraft | undefined {
  if (node.id === id) {
    return node;
  }
  if (node.kind === 'leaf') {
    return undefined;
  }
  for (const child of node.children) {
    const found = findNode(child, id);
    if (found !== undefined) {
      return found;
    }
  }
  return undefined;
}

export function walkNodes(node: NodeDraft): NodeDraft[] {
  return node.kind === 'leaf' ? [node] : [node, ...node.children.flatMap(walkNodes)];
}

export function countLeaves(node: NodeDraft): number {
  return walkNodes(node).filter((item) => item.kind === 'leaf').length;
}

export function depthOf(node: NodeDraft): number {
  const own = node.negate ? 1 : 0;
  if (node.kind === 'leaf') {
    return own;
  }
  return own + 1 + Math.max(0, ...node.children.map(depthOf));
}

export function fitsLimits(root: GroupDraft, limits: ConditionLimits): boolean {
  return depthOf({ ...root, negate: false }) <= limits.maxDepth && countLeaves(root) <= limits.maxLeaves;
}

export function numberNodes(root: GroupDraft): ConditionNumbering {
  const leaves = new Map<string, number>();
  const groups = new Map<string, number>();
  for (const node of walkNodes(root)) {
    if (node.kind === 'leaf') {
      leaves.set(node.id, leaves.size + 1);
    } else if (node.id !== root.id) {
      groups.set(node.id, groups.size + 1);
    }
  }
  return { leaves, groups };
}

export function changedNodeIds(previous: GroupDraft, next: GroupDraft): string[] {
  const current = new Map(walkNodes(next).map((node) => [node.id, node]));
  return walkNodes(previous)
    .filter((node) => current.get(node.id) !== node)
    .map((node) => node.id);
}
